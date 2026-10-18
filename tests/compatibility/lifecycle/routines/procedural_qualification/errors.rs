#[test]
fn should_restore_routine_scoped_settings_after_a_function_error() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("routine_scoped_setting_error").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE FUNCTION compat_scoped_divide_by_zero() RETURNS INT \
                   LANGUAGE SQL SET application_name = 'inside-failing-function' \
                   AS $$ SELECT 1 / 0 $$",
                "SET application_name = 'outside-failing-function'",
            ],
        )
        .await
        .expect("create a function whose scoped setting surrounds an error");

        // Act
        let error = client
            .query("SELECT compat_scoped_divide_by_zero()", &[])
            .await
            .expect_err("the function body should raise division by zero");
        let application_name: String = client
            .query_one("SHOW application_name", &[])
            .await
            .expect("read the caller setting after the function error")
            .try_get(0)
            .expect("caller application name");

        // Assert
        assert_eq!(db_error(&error).code().code(), "22012");
        assert_eq!(application_name, "outside-failing-function");

        drop(client);
        server.shutdown(connection).await;
    });
}
