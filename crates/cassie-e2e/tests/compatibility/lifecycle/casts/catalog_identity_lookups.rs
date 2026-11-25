#[test]
fn should_report_missing_regclass_cast_with_undefined_table_sqlstate() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("missing_regclass_cast_sqlstate").await;
        let (client, connection) = server.connect().await;

        // Act
        let error = client
            .query("SELECT 'issue_818_missing_oid_relation'::regclass", &[])
            .await
            .expect_err("a missing regclass cast must raise undefined_table");

        // Assert
        assert_eq!(db_error(&error).code().code(), "42P01");
        let recovered = client
            .query_one("SELECT 42", &[])
            .await
            .expect("connection remains usable after a missing regclass input");
        assert_eq!(recovered.get::<_, i32>(0), 42);

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_report_missing_regprocedure_cast_with_undefined_function_sqlstate() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("missing_regprocedure_cast_sqlstate").await;
        let (client, connection) = server.connect().await;

        // Act
        let error = client
            .query(
                "SELECT 'issue_818_missing_oid_function(integer)'::regprocedure",
                &[],
            )
            .await
            .expect_err("a missing regprocedure cast must raise undefined_function");

        // Assert
        assert_eq!(db_error(&error).code().code(), "42883");
        let recovered = client
            .query_one("SELECT 42", &[])
            .await
            .expect("connection remains usable after a missing regprocedure input");
        assert_eq!(recovered.get::<_, i32>(0), 42);

        drop(client);
        server.shutdown(connection).await;
    });
}
