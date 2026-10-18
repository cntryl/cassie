#[test]
fn should_rollback_exception_block_effects_return_from_the_handler() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("plpgsql_exception_subtransaction").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_plpgsql_exception_rows (id INT PRIMARY KEY)",
                "CREATE FUNCTION compat_plpgsql_catch_raise(input_id INT) RETURNS INT \
                   LANGUAGE plpgsql AS $function$ \
                   BEGIN \
                     BEGIN \
                       INSERT INTO compat_plpgsql_exception_rows VALUES (input_id); \
                       RAISE EXCEPTION 'rollback this block'; \
                     EXCEPTION WHEN raise_exception THEN \
                       RETURN 7; \
                     END; \
                   END; \
                   $function$",
            ],
        )
        .await
        .expect("create PL/pgSQL function with an exception subblock");

        // Act
        let returned: i32 = client
            .query_one("SELECT compat_plpgsql_catch_raise(4)", &[])
            .await
            .expect("invoke the PL/pgSQL exception handler")
            .try_get(0)
            .expect("handler return value");
        let rows = client
            .query("SELECT id FROM compat_plpgsql_exception_rows", &[])
            .await
            .expect("read rows after the handled exception");

        // Assert
        assert_eq!(returned, 7);
        assert!(
            rows.is_empty(),
            "the exception block must roll back its insert"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
