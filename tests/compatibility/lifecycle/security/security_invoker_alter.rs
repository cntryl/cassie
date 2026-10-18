use super::*;

#[test]
fn should_apply_security_invoker_after_altering_definer_function() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("alter_function_security_invoker").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE ROLE compat_832_security_invoker NOLOGIN",
                "CREATE TABLE compat_832_security_secret (payload TEXT)",
                "INSERT INTO compat_832_security_secret VALUES ('owner-value')",
                "CREATE FUNCTION public.compat_832_read_secret() RETURNS TEXT \
                   LANGUAGE SQL SECURITY DEFINER \
                   AS $$ SELECT payload FROM public.compat_832_security_secret $$",
                "REVOKE ALL ON FUNCTION public.compat_832_read_secret() FROM PUBLIC",
                "GRANT EXECUTE ON FUNCTION public.compat_832_read_secret() \
                   TO compat_832_security_invoker",
                "SET ROLE compat_832_security_invoker",
            ],
        )
        .await
        .expect("create a definer routine and assume its restricted caller role");
        let definer_value: String = client
            .query_one("SELECT public.compat_832_read_secret()", &[])
            .await
            .expect("the SECURITY DEFINER routine reads its owner's protected table")
            .try_get(0)
            .expect("protected value returned by the definer routine");
        client
            .batch_execute(
                "RESET ROLE; \
                 ALTER FUNCTION public.compat_832_read_secret() SECURITY INVOKER",
            )
            .await
            .expect("alter the routine back to caller privileges");
        let security_definer_flag: bool = client
            .query_one(
                "SELECT prosecdef FROM pg_catalog.pg_proc \
                 WHERE oid = 'public.compat_832_read_secret()'::regprocedure",
                &[],
            )
            .await
            .expect("read the routine's security mode from pg_proc")
            .try_get(0)
            .expect("pg_proc prosecdef flag");
        client
            .batch_execute("SET ROLE compat_832_security_invoker")
            .await
            .expect("assume the restricted caller role again");

        // Act
        let invoker_error = client
            .query_one("SELECT public.compat_832_read_secret()", &[])
            .await
            .expect_err("the SECURITY INVOKER routine uses the caller's table privileges");
        client
            .batch_execute("RESET ROLE")
            .await
            .expect("restore the administrative role after the denied call");

        // Assert
        drop(client);
        server.shutdown(connection).await;
        assert_eq!(definer_value, "owner-value");
        assert!(!security_definer_flag);
        assert_eq!(db_error(&invoker_error).code().code(), "42501");
    });
}
