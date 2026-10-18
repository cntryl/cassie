use super::*;

#[test]
fn should_cast_box_area_through_a_sql_defined_builtin_cast() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("sql_defined_builtin_cast").await;
        let (client, connection) = server.connect().await;
        let ready = client
            .query_one("SELECT 1", &[])
            .await
            .expect("fixture connection should execute a baseline query");
        assert_eq!(ready.get::<_, i32>(0), 1);

        // Act
        client
            .batch_execute(
                "CREATE CAST (box AS double precision) \
                 WITH FUNCTION pg_catalog.area(box)",
            )
            .await
            .expect("CREATE CAST should register the SQL-defined built-in cast");
        let result = client
            .query_one("SELECT '(0,0),(2,3)'::box::double precision", &[])
            .await
            .expect("cast box area to double precision");

        // Assert
        assert_eq!(result.columns()[0].type_().oid(), 701);
        assert_eq!(result.get::<_, f64>(0), 6.0);

        drop(client);
        server.shutdown(connection).await;
    });
}
