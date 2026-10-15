use super::*;

#[test]
fn should_preserve_record_null_comparison_semantics() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("record_null_comparison_semantics").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_record_comparison",
                "CREATE TYPE compat_record_comparison.pair_value AS \
                   (key_value INTEGER, payload INTEGER)",
            ],
        )
        .await
        .expect("create a named composite type for NULL comparison semantics");

        // Act
        let comparisons = client
            .query_one(
                "SELECT ROW(1, NULL) = ROW(1, 2), \
                        ROW(1, NULL) < ROW(1, 2), \
                        (ROW(1, NULL)::compat_record_comparison.pair_value) = \
                          (ROW(1, 2)::compat_record_comparison.pair_value), \
                        (ROW(1, NULL)::compat_record_comparison.pair_value) < \
                          (ROW(1, 2)::compat_record_comparison.pair_value), \
                        (ROW(1, NULL)::compat_record_comparison.pair_value) > \
                          (ROW(1, 2)::compat_record_comparison.pair_value), \
                        (ROW(1, NULL)::compat_record_comparison.pair_value) \
                          IS DISTINCT FROM \
                          (ROW(1, 2)::compat_record_comparison.pair_value)",
                &[],
            )
            .await
            .expect("compare anonymous records and named composite values");

        // Assert
        assert!(comparisons
            .columns()
            .iter()
            .all(|column| column.type_().oid() == 16));
        assert_eq!(comparisons.get::<_, Option<bool>>(0), None);
        assert_eq!(comparisons.get::<_, Option<bool>>(1), None);
        assert!(!comparisons.get::<_, bool>(2));
        assert!(!comparisons.get::<_, bool>(3));
        assert!(comparisons.get::<_, bool>(4));
        assert!(comparisons.get::<_, bool>(5));

        drop(client);
        server.shutdown(connection).await;
    });
}
