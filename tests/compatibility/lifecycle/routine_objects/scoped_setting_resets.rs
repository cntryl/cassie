use super::*;

#[test]
fn should_restore_caller_application_name_after_function_reset() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("routine_scoped_setting_reset_one").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE FUNCTION public.compat_832_reset_one() RETURNS TEXT \
                   LANGUAGE SQL STABLE SET application_name = 'function-one' \
                   AS $$ SELECT current_setting('application_name') $$",
                "ALTER FUNCTION public.compat_832_reset_one() RESET application_name",
                "SET application_name = 'caller-value'",
            ],
        )
        .await
        .expect("reset the routine setting and establish a caller application name");

        // Act
        let reset_parameter: String = client
            .query_one("SELECT public.compat_832_reset_one()", &[])
            .await
            .expect("invoke the function after resetting application_name")
            .try_get(0)
            .expect("application_name observed by the reset function");
        let caller_application_name: String = client
            .query_one("SHOW application_name", &[])
            .await
            .expect("read caller application_name after function invocation")
            .try_get(0)
            .expect("caller application_name");

        // Assert
        assert_eq!(reset_parameter, "caller-value");
        assert_eq!(caller_application_name, "caller-value");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_restore_caller_settings_after_routine_reset_all() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("routine_scoped_setting_reset_all").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_832_caller_path",
                "CREATE FUNCTION public.compat_832_reset_all() RETURNS TEXT \
                   LANGUAGE SQL STABLE \
                   SET application_name = 'function-all' \
                   SET search_path = pg_catalog \
                   AS $$ SELECT current_setting('application_name') || '|' || \
                             current_setting('search_path') $$",
                "ALTER FUNCTION public.compat_832_reset_all() RESET ALL",
                "SET application_name = 'caller-value'",
                "SET search_path TO compat_832_caller_path, public",
            ],
        )
        .await
        .expect("reset all routine settings and establish caller values");

        // Act
        let reset_all: String = client
            .query_one("SELECT public.compat_832_reset_all()", &[])
            .await
            .expect("invoke the function after resetting all routine settings")
            .try_get(0)
            .expect("settings observed by the reset-all function");
        let caller_application_name: String = client
            .query_one("SHOW application_name", &[])
            .await
            .expect("read caller application_name after function invocation")
            .try_get(0)
            .expect("caller application_name");
        let caller_search_path: String = client
            .query_one("SHOW search_path", &[])
            .await
            .expect("read caller search_path after function invocation")
            .try_get(0)
            .expect("caller search_path");

        // Assert
        assert_eq!(reset_all, "caller-value|compat_832_caller_path, public");
        assert_eq!(caller_application_name, "caller-value");
        assert_eq!(caller_search_path, "compat_832_caller_path, public");

        drop(client);
        server.shutdown(connection).await;
    });
}
