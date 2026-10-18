#[test]
fn should_parse_quoted_composite_literals_with_empty_null_fields() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("composite_literal_io_wave6").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE TYPE compat_pair_literal_wave6 AS \
                 (name TEXT, quantity INT, note TEXT)",
            )
            .await
            .expect("create a named composite type for text input/output");

        // Act
        let row = client
            .query_one(
                "SELECT '(\"fuzzy dice\",42,)'::compat_pair_literal_wave6::TEXT, \
                 (ROW('fuzzy dice', 42, NULL)::compat_pair_literal_wave6).name, \
                 (ROW('fuzzy dice', 42, NULL)::compat_pair_literal_wave6).quantity, \
                 (ROW('fuzzy dice', 42, NULL)::compat_pair_literal_wave6).note IS NULL",
                &[],
            )
            .await
            .expect("parse a quoted composite literal and preserve empty-field NULL rules");

        // Assert
        assert_eq!(row.get::<_, String>(0), "(\"fuzzy dice\",42,)");
        assert_eq!(row.get::<_, String>(1), "fuzzy dice");
        assert_eq!(row.get::<_, i32>(2), 42);
        assert!(row.get::<_, bool>(3));

        drop(client);
        server.shutdown(connection).await;
    });
}
