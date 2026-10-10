// Finite prepared and Data-applied publication recovery across two cold startups.
use super::*;
use crate::vector_publication_support::{
    assert_cleanup, raw_rows, row_payloads, PendingBackfill, RawRecords,
};

fn fixture(label: &str) -> (Cassie, String) {
    use_local_storage();
    let path = data_dir(label);
    let cassie = start_vector_cassie(&path);
    create_table(&cassie, "recover_backfill", false);
    run(
        &cassie,
        "INSERT INTO recover_backfill (id, body) VALUES ('a', 'hello'), ('b', 'world')",
        vec![],
    );
    (cassie, path)
}

fn version(cassie: &Cassie) -> (u64, u64) {
    (
        cassie
            .midge
            .collection_generation("recover_backfill")
            .expect("collection generation"),
        cassie.midge.data_epoch().expect("Data epoch"),
    )
}

fn assert_metadata(cassie: &Cassie, expected: &PendingBackfill) {
    let collection = &expected.vector_index.collection;
    let vector = cassie
        .midge
        .get_vector_index(collection, "embedding")
        .expect("vector metadata");
    assert!(
        vector.as_ref() == Some(&expected.vector_index),
        "complete durable vector metadata"
    );
    assert!(
        cassie
            .catalog
            .get_vector_index(collection, "embedding")
            .as_ref()
            == Some(&expected.vector_index),
        "complete hydrated vector catalog metadata"
    );
    let indexes = cassie.midge.list_indexes().expect("durable SQL indexes");
    let actual = indexes
        .into_iter()
        .filter(|index| &index.collection == collection)
        .collect::<Vec<_>>();
    let expected_sql = expected.sql_index.iter().cloned().collect::<Vec<_>>();
    assert!(
        actual == expected_sql,
        "exact SQL index presence and metadata for SQL or REST publication"
    );
    // Catalog hydration deliberately hides physical storage identities from
    // logical metadata (src/app/hydration.rs::hydrate_operational_metadata).
    let mut expected_catalog = expected_sql;
    for index in &mut expected_catalog {
        index.options.remove("__cassie_relation_id");
        index.options.remove("__cassie_storage_id");
    }
    assert!(
        cassie.catalog.list_indexes(collection) == expected_catalog,
        "exact hydrated SQL index catalog for SQL or REST publication"
    );
}

fn assert_unpublished_metadata(cassie: &Cassie) {
    assert!(cassie
        .midge
        .get_vector_index("recover_backfill", "embedding")
        .expect("unpublished vector metadata")
        .is_none());
    assert!(cassie
        .catalog
        .get_vector_index("postgres.public.recover_backfill", "embedding")
        .is_none());
    assert_eq!(
        cassie
            .midge
            .list_indexes()
            .expect("unpublished SQL metadata"),
        [] as [cassie::catalog::IndexMeta; 0]
    );
    assert_eq!(
        cassie
            .catalog
            .list_indexes("postgres.public.recover_backfill"),
        [] as [cassie::catalog::IndexMeta; 0]
    );
}

fn assert_two_cold_startups(
    path: &str,
    expected: PendingBackfill,
    initial: (u64, u64),
    mut first_raw: Option<RawRecords>,
) {
    for _ in 0..2 {
        let mut cassie = new_vector_cassie(path);
        let provider = std::sync::Arc::new(crate::backfill_provider::FailSecondProvider::default());
        cassie.embedding_provider = provider.clone();
        cassie.startup().expect("cold publication startup");
        // These are read-only oracles. In particular, no manual replay can mask
        // pending records that startup itself should already have retired.
        assert_cleanup(&cassie);
        assert!(
            row_payloads(&cassie, "recover_backfill") == expected.rows,
            "startup restores exact complete staged row payloads"
        );
        let rows = raw_rows(&cassie);
        if let Some(first_raw) = first_raw.as_ref() {
            assert!(
                &rows == first_raw,
                "second cold startup preserves canonical row bytes"
            );
        } else {
            first_raw = Some(rows);
        }
        assert_eq!(
            version(&cassie),
            (initial.0 + 1, initial.1 + 1),
            "backfill changes generation and Data epoch exactly once across both startups"
        );
        assert_metadata(&cassie, &expected);
        assert_eq!(
            provider.call_count(),
            0,
            "startup never contacts embedding provider"
        );
        drop(provider);
        cassie.shutdown();
        drop(cassie);
    }
    drop(first_raw);
    drop(expected);
    std::fs::remove_dir_all(path).expect("strict repeated-restart cleanup after owners drop");
    assert!(!std::path::Path::new(path).exists());
}

#[test]
fn should_recover_prepared_sql_vector_publication_across_two_cold_startups() {
    // Arrange
    let (cassie, path) = fixture("vector_backfill_prepared_two_cold_startups");
    let initial = version(&cassie);
    let before = row_payloads(&cassie, "recover_backfill");
    let before_raw = raw_rows(&cassie);
    let session = cassie.create_session("tester", None);
    cassie::midge::adapter::set_index_publication_failure_point(true);
    // Act
    let result = cassie.execute_sql(&session,
        "CREATE INDEX recover_idx ON recover_backfill USING vector (embedding) WITH (source_field = body)", vec![]);
    cassie::midge::adapter::set_index_publication_failure_point(false);
    // Assert
    let error = result.expect_err("prepared publication interruption");
    assert!(error
        .to_string()
        .contains("recoverable vector index publication"));
    let expected = PendingBackfill::capture(&cassie, initial.0, true);
    expected.assert_applied_marker(&cassie, false);
    assert!(
        row_payloads(&cassie, "recover_backfill") == before,
        "unapplied rows unchanged"
    );
    assert!(
        raw_rows(&cassie) == before_raw,
        "unapplied canonical row bytes unchanged"
    );
    assert_eq!(version(&cassie), initial, "unapplied version unchanged");
    assert_unpublished_metadata(&cassie);
    drop(error);
    drop(session);
    drop(before);
    drop(before_raw);
    cassie.shutdown();
    drop(cassie);
    assert_two_cold_startups(&path, expected, initial, None);
}

#[test]
fn should_recover_data_applied_rest_vector_publication_across_two_cold_startups() {
    // Arrange
    let _guard = cassie::midge::adapter::document_write_failure_point_test_guard();
    let (cassie, path) = fixture("vector_backfill_applied_two_cold_startups");
    let initial = version(&cassie);
    let before = row_payloads(&cassie, "recover_backfill");
    cassie::midge::adapter::set_document_write_failure_point(Some(
        cassie::midge::adapter::DocumentWriteFailurePoint::VectorState,
    ));
    // Act
    let result = rest_index(&cassie, "recover_backfill");
    cassie::midge::adapter::set_document_write_failure_point(None);
    // Assert
    let error = result.expect_err("Data-applied sidecar interruption");
    assert!(error
        .to_string()
        .contains("recoverable vector index publication"));
    let expected = PendingBackfill::capture(&cassie, initial.0, false);
    expected.assert_applied_marker(&cassie, true);
    for ((before_id, before_payload), (after_id, after_payload)) in
        before.iter().zip(&expected.rows)
    {
        assert_eq!(
            before_id, after_id,
            "backfill preserves physical row identity"
        );
        for field in ["id", "body"] {
            assert!(
                before_payload[field] == after_payload[field],
                "backfill preserves source payload"
            );
        }
        assert!(before_payload
            .get("embedding")
            .is_none_or(serde_json::Value::is_null));
        assert_eq!(
            after_payload["embedding"]
                .as_array()
                .expect("prepared vector")
                .len(),
            3
        );
    }
    assert!(
        row_payloads(&cassie, "recover_backfill") == expected.rows,
        "every prepared row is already applied atomically"
    );
    assert_eq!(
        version(&cassie),
        (initial.0 + 1, initial.1 + 1),
        "Data application advances generation and epoch once before recovery"
    );
    assert_unpublished_metadata(&cassie);
    let applied_raw = raw_rows(&cassie);
    drop(error);
    drop(before);
    cassie.shutdown();
    drop(cassie);
    assert_two_cold_startups(&path, expected, initial, Some(applied_raw));
}
