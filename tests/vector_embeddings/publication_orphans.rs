// Orphan-only cleanup uses actual cold startup, without pending-intent replay.
use super::*;
use crate::vector_publication_support::{
    raw_rows, row_payloads, OrphanPublicationArtifacts, RawRecords,
};

fn version(cassie: &Cassie) -> (u64, u64) {
    (
        cassie
            .midge
            .collection_generation("recover_backfill")
            .expect("collection generation"),
        cassie.midge.data_epoch().expect("Data epoch"),
    )
}

fn assert_stored_rows(
    cassie: &Cassie,
    payloads: &[(String, serde_json::Value)],
    rows: &RawRecords,
    initial: (u64, u64),
) {
    assert_eq!(
        row_payloads(cassie, "recover_backfill").as_slice(),
        payloads
    );
    assert_eq!(&raw_rows(cassie), rows, "exact canonical row bytes");
    assert_eq!(version(cassie), initial, "cleanup does not publish Data");
}

fn assert_rows_and_unpublished_metadata(cassie: &Cassie) {
    assert_eq!(
        run(
            cassie,
            "SELECT id, body, embedding FROM recover_backfill ORDER BY id",
            vec![],
        ),
        vec![
            vec![
                Value::String("a".to_string()),
                Value::String("hello".to_string()),
                Value::Null,
            ],
            vec![
                Value::String("b".to_string()),
                Value::String("world".to_string()),
                Value::Null,
            ],
        ],
        "orphan payloads never replace or add authoritative rows"
    );
    assert!(cassie
        .midge
        .get_vector_index("recover_backfill", "embedding")
        .expect("durable vector metadata")
        .is_none());
    assert!(cassie
        .catalog
        .get_vector_index("postgres.public.recover_backfill", "embedding")
        .is_none());
    assert_eq!(
        cassie.midge.list_indexes().expect("durable SQL metadata"),
        [] as [cassie::catalog::IndexMeta; 0]
    );
    assert_eq!(
        cassie
            .catalog
            .list_indexes("postgres.public.recover_backfill"),
        [] as [cassie::catalog::IndexMeta; 0]
    );
}

#[test]
fn should_remove_orphan_vector_publication_artifacts_across_two_cold_startups() {
    // Arrange
    use_local_storage();
    let path = data_dir("vector_publication_orphans");
    let cassie = start_vector_cassie(&path);
    create_table(&cassie, "recover_backfill", false);
    run(
        &cassie,
        "INSERT INTO recover_backfill (id, body) VALUES ('a', 'hello'), ('b', 'world')",
        vec![],
    );
    let payloads = row_payloads(&cassie, "recover_backfill");
    let rows = raw_rows(&cassie);
    let artifacts = OrphanPublicationArtifacts::seed(&cassie);
    let initial = version(&cassie);
    assert_stored_rows(&cassie, &payloads, &rows, initial);
    assert_rows_and_unpublished_metadata(&cassie);
    cassie.shutdown();
    drop(cassie);

    // Act
    for startup in 0..2 {
        let mut cassie = new_vector_cassie(&path);
        if startup == 0 {
            artifacts.assert_present(&cassie);
        } else {
            artifacts.assert_absent(&cassie);
        }
        assert_stored_rows(&cassie, &payloads, &rows, initial);
        let provider = std::sync::Arc::new(crate::backfill_provider::FailSecondProvider::default());
        cassie.embedding_provider = provider.clone();
        cassie.startup().expect("orphan-only cold startup");

        // Assert
        // No manual replay or cleanup can hide leftovers at this boundary.
        artifacts.assert_absent(&cassie);
        assert_stored_rows(&cassie, &payloads, &rows, initial);
        assert_rows_and_unpublished_metadata(&cassie);
        assert_eq!(provider.call_count(), 0, "startup never embeds orphan rows");
        drop(provider);
        cassie.shutdown();
        drop(cassie);
    }
    drop(artifacts);
    drop(rows);
    drop(payloads);
    std::fs::remove_dir_all(&path).expect("strict cleanup after all owners retire");
    assert!(!std::path::Path::new(&path).exists());
}
