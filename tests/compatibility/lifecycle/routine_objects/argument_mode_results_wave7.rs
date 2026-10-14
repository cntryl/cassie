use super::*;

#[test]
fn should_return_out_arguments_as_call_result_columns() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("routine_out_argument_result").await;
        let (client, connection) = server.connect().await;
        let baseline = client
            .query_one("SELECT 1", &[])
            .await
            .expect("the routine session should be ready before creating the procedure");
        assert_eq!(baseline.get::<_, i32>(0), 1);
        client
            .batch_execute(
                "CREATE PROCEDURE compat_out_argument(IN input_value INT, OUT output_value INT) \
                   LANGUAGE SQL AS $$ SELECT input_value * 2 $$",
            )
            .await
            .expect("create a SQL procedure with an OUT argument");

        // Act
        let rows = client
            .query("CALL compat_out_argument(21, NULL)", &[])
            .await
            .expect("top-level CALL should return the procedure's OUT argument row");

        // Assert
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].columns().len(), 1);
        assert_eq!(rows[0].columns()[0].name(), "output_value");
        assert_eq!(rows[0].columns()[0].type_().oid(), 23);
        assert_eq!(rows[0].get::<_, i32>(0), 42);

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_return_inout_arguments_after_procedure_updates_them() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("routine_inout_argument_result").await;
        let (client, connection) = server.connect().await;
        let baseline = client
            .query_one("SELECT 1", &[])
            .await
            .expect("the routine session should be ready before creating the procedure");
        assert_eq!(baseline.get::<_, i32>(0), 1);
        client
            .batch_execute(
                "CREATE PROCEDURE compat_inout_argument(INOUT value INT) \
                   LANGUAGE SQL AS $$ SELECT value + 1 $$",
            )
            .await
            .expect("create a SQL procedure with an INOUT argument");

        // Act
        let rows = client
            .query("CALL compat_inout_argument(41)", &[])
            .await
            .expect("top-level CALL should return the updated INOUT argument");

        // Assert
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].columns().len(), 1);
        assert_eq!(rows[0].columns()[0].name(), "value");
        assert_eq!(rows[0].columns()[0].type_().oid(), 23);
        assert_eq!(rows[0].get::<_, i32>(0), 42);

        drop(client);
        server.shutdown(connection).await;
    });
}
