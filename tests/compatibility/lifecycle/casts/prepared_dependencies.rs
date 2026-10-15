use super::*;

async fn create_cast_dependency_fixture(client: &tokio_postgres::Client) {
    for statement in [
        "CREATE TYPE compat_cast_source AS (value INTEGER)",
        "CREATE TYPE compat_cast_target AS (value BIGINT)",
        "CREATE FUNCTION compat_cast_source_to_target(compat_cast_source) \
         RETURNS compat_cast_target LANGUAGE SQL IMMUTABLE STRICT AS \
         $$ SELECT ROW(($1).value::BIGINT)::compat_cast_target $$",
        "CREATE CAST (compat_cast_source AS compat_cast_target) \
         WITH FUNCTION compat_cast_source_to_target(compat_cast_source) AS ASSIGNMENT",
    ] {
        client
            .batch_execute(statement)
            .await
            .expect("create the custom cast and its declared SQL dependencies");
    }
}

fn cast_dependency_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread Tokio runtime")
}

#[test]
fn should_revalidate_cast_dependencies_given_a_prepared_cast_when_its_cast_object_is_dropped() {
    // Arrange
    let runtime = cast_dependency_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("prepared_cast_drop").await;
        let (client, connection) = server.connect().await;
        create_cast_dependency_fixture(&client).await;
        client
            .batch_execute(
                "PREPARE compat_prepared_cast AS \
                 SELECT ((ROW(7)::compat_cast_source)::compat_cast_target).value",
            )
            .await
            .expect("prepare an expression that depends on the declared cast");

        // Act
        client
            .batch_execute("DROP CAST (compat_cast_source AS compat_cast_target)")
            .await
            .expect("drop the cast after the prepared statement has been analyzed");
        let prepared_value: i64 = client
            .query_one("EXECUTE compat_prepared_cast", &[])
            .await
            .expect("the cached PostgreSQL plan retains its resolved cast function")
            .get(0);
        let fresh_resolution = client
            .query_one(
                "SELECT ((ROW(7)::compat_cast_source)::compat_cast_target).value",
                &[],
            )
            .await;

        // Assert
        assert_eq!(prepared_value, 7);
        assert_eq!(
            db_error(
                &fresh_resolution.expect_err("a new query no longer resolves the dropped cast")
            )
            .code()
            .code(),
            "42846"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_restore_cast_resolution_given_rolled_back_cast_ddl_when_a_prepared_statement_is_reused() {
    // Arrange
    let runtime = cast_dependency_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("prepared_cast_rollback").await;
        let (client, connection) = server.connect().await;
        create_cast_dependency_fixture(&client).await;
        client
            .batch_execute(
                "PREPARE compat_prepared_cast_rollback AS \
                 SELECT ((ROW(11)::compat_cast_source)::compat_cast_target).value",
            )
            .await
            .expect("prepare the cast expression before transactional DDL");

        // Act
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin a transaction for the cast removal");
        client
            .batch_execute("DROP CAST (compat_cast_source AS compat_cast_target)")
            .await
            .expect("drop the custom cast inside the transaction");
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("restore the cast object with transaction rollback");
        let value: i64 = client
            .query_one("EXECUTE compat_prepared_cast_rollback", &[])
            .await
            .expect("reuse the prepared expression after rollback")
            .get(0);

        // Assert
        assert_eq!(value, 11);

        drop(client);
        server.shutdown(connection).await;
    });
}
