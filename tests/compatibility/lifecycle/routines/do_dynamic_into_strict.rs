use super::*;

#[test]
fn should_reject_multiple_dynamic_query_rows_into_strict_with_sqlstate() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("do_dynamic_into_strict_shape").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_do_strict_source (id INT PRIMARY KEY)",
                "CREATE TABLE compat_do_strict_effects (id INT)",
                "INSERT INTO compat_do_strict_source VALUES (1), (2)",
            ],
        )
        .await
        .expect("create the source rows and observable earlier DO effect");

        // Act
        let strict_error = client
            .batch_execute(
                "DO $block$ \
                   DECLARE selected_id INT; \
                   BEGIN \
                     INSERT INTO compat_do_strict_effects VALUES (99); \
                     EXECUTE 'SELECT id FROM compat_do_strict_source ORDER BY id' \
                       INTO STRICT selected_id; \
                     INSERT INTO compat_do_strict_effects VALUES (selected_id); \
                   END; \
                   $block$",
            )
            .await
            .expect_err("dynamic EXECUTE INTO STRICT must reject a multirow result");
        let effects = client
            .query("SELECT id FROM compat_do_strict_effects ORDER BY id", &[])
            .await
            .expect("the session must remain usable after the failed anonymous block");

        // Assert
        assert_eq!(db_error(&strict_error).code().code(), "P0003");
        assert!(
            effects.is_empty(),
            "the strict cardinality error must roll back all effects of the DO statement"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
