use super::*;

#[test]
fn should_resolve_schema_qualified_builtin_calls() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue830_qualified_builtin_call").await;
        let (client, connection) = server.connect().await;

        // Act
        let row = client
            .query_one("SELECT pg_catalog.lower('MiXeD')", &[])
            .await
            .expect("resolve the pg_catalog-qualified lower routine");

        // Assert
        assert_eq!(row.columns()[0].type_().oid(), 25);
        assert_eq!(row.get::<_, String>(0), "mixed");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_resolve_schema_qualified_now_builtin() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue830_qualified_now_builtin").await;
        let (client, connection) = server.connect().await;

        // Act
        let row = client
            .query_one("SELECT pg_catalog.now() IS NOT NULL", &[])
            .await
            .expect("resolve the schema-qualified stable now routine");

        // Assert
        assert_eq!(row.columns()[0].type_().oid(), 16);
        assert!(row.get::<_, bool>(0));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_accept_signature_qualified_alter_function_cost() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue830_alter_function_cost").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("BEGIN")
            .await
            .expect("start a transaction for the routine catalog change");

        // Act
        client
            .batch_execute("ALTER FUNCTION pg_catalog.lower(text) COST 42")
            .await
            .expect("alter the selected overload's routine cost");
        let row = client
            .query_one("SELECT pg_catalog.lower('MiXeD')", &[])
            .await
            .expect("call the routine after changing its cost attribute");
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back the catalog change");

        // Assert
        assert_eq!(row.columns()[0].type_().oid(), 25);
        assert_eq!(row.get::<_, String>(0), "mixed");

        drop(client);
        server.shutdown(connection).await;
    });
}
