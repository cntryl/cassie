use super::support_read_equivalence::{sql, with_fixture};
use super::support_sql;
use cassie::app::Cassie;

fn document_snapshot(cassie: &Cassie, collection: &str) -> Vec<(String, serde_json::Value)> {
    cassie
        .midge
        .scan_documents(collection)
        .expect("scan derived rows")
        .into_iter()
        .map(|document| (document.id, document.payload))
        .collect()
}

#[test]
fn should_reject_nonfinite_projection_refresh_before_replacing_active_output() {
    // Arrange
    with_fixture("nonfinite_projection_refresh", |cassie, session| {
        sql(
            cassie,
            session,
            "CREATE TABLE nf_projection_source (k TEXT, v FLOAT)",
        );
        sql(
            cassie,
            session,
            "INSERT INTO nf_projection_source VALUES ('a', 1.0)",
        );
        sql(
                cassie,
                session,
                "CREATE MATERIALIZED PROJECTION nf_projection AS SELECT k, v * 10 AS big FROM nf_projection_source",
            );
        let before_metadata = cassie
            .catalog
            .get_materialized_projection("nf_projection")
            .expect("projection metadata");
        let active_version = before_metadata
            .active_version
            .clone()
            .expect("active projection version");
        let output_collection = before_metadata
            .active_output_collection()
            .expect("active output collection")
            .to_string();
        let before_rows = document_snapshot(cassie, &output_collection);
        let source_collection =
            support_sql::canonical_test_collection(cassie, "nf_projection_source");
        let source_row = cassie
            .midge
            .scan_documents(&source_collection)
            .expect("scan source rows")
            .into_iter()
            .next()
            .expect("source row");
        cassie
            .midge
            .put_document(
                &source_collection,
                Some(source_row.id),
                serde_json::json!({"k":"a","v":1e308}),
            )
            .expect("replace source with finite large float");

        // Act
        let refresh = cassie.execute_sql(
            session,
            "REFRESH MATERIALIZED PROJECTION nf_projection",
            vec![],
        );

        // Assert
        let error = refresh.expect_err("non-finite projection output must be rejected");
        assert!(error.to_string().contains("non-finite"), "{error}");
        let after_metadata = cassie
            .catalog
            .get_materialized_projection("nf_projection")
            .expect("projection metadata after failed refresh");
        assert_eq!(
            after_metadata.active_version.as_deref(),
            Some(active_version.as_str())
        );
        assert_eq!(document_snapshot(cassie, &output_collection), before_rows);
        assert_eq!(before_rows[0].1["big"], serde_json::json!(10.0));
    });
}
