use cassie::app::Cassie;
use cassie::midge::adapter::set_projection_hash_maintenance_failure_point;

#[path = "support/sql.rs"]
mod support;
use support::{canonical_test_collection, data_dir, with_fallback};

#[test]
fn should_retry_projection_hash_debt_after_restart() {
    // Arrange
    with_fallback();
    let path = data_dir("projection_hash_debt_recovery");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE projection_hash_debt_docs (title TEXT)",
                vec![],
            )
            .expect("create table");
        let collection = canonical_test_collection(&cassie, "projection_hash_debt_docs");
        cassie
            .midge
            .put_fresh_documents(
                &collection,
                vec![(
                    Some("doc-1".to_string()),
                    serde_json::json!({"title": "before"}),
                )],
            )
            .expect("seed current projection hashes");
        assert!(cassie
            .midge
            .root_hash(&collection)
            .expect("read current root")
            .is_some());

        // Act
        set_projection_hash_maintenance_failure_point(true);
        cassie
            .midge
            .put_fresh_documents(
                &collection,
                vec![(
                    Some("doc-2".to_string()),
                    serde_json::json!({"title": "alpha"}),
                )],
            )
            .expect("durable write must not return a maintenance failure");
        assert!(cassie
            .midge
            .has_projection_hash_maintenance_debt(&collection)
            .expect("read durable debt"));
        assert!(cassie
            .midge
            .root_hash(&collection)
            .expect("read stale root")
            .is_none());
        drop(cassie);

        let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
        restarted.startup().expect("recover projection hashes");

        // Assert
        assert!(!restarted
            .midge
            .has_projection_hash_maintenance_debt(&collection)
            .expect("debt should be cleared after recovery"));
        assert!(restarted
            .midge
            .root_hash(&collection)
            .expect("read recovered root")
            .is_some());
    });

    let _ = std::fs::remove_dir_all(path);
}
