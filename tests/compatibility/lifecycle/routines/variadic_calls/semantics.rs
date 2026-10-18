use super::*;

#[test]
fn should_expand_variadic_arrays_for_builtin_string_functions() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("builtin_variadic_string_calls").await;
        let (client, connection) = server.connect().await;

        // Act
        let result = client
            .query_one(
                "SELECT concat(VARIADIC '{north,star}'::TEXT[]) AS joined, \
                        format('<%s><%s>', VARIADIC '{north,star}'::TEXT[]) AS formatted",
                &[],
            )
            .await;

        // Assert
        assert!(
            result.is_ok(),
            "PostgreSQL VARIADIC calls should expand array elements into function arguments: \
             {result:?}"
        );
        let row = result.expect("read variadic string function results");
        assert_eq!(row.columns()[0].type_().oid(), 25);
        assert_eq!(row.columns()[1].type_().oid(), 25);
        assert_eq!(row.get::<_, String>(0), "northstar");
        assert_eq!(row.get::<_, String>(1), "<north><star>");

        drop(client);
        server.shutdown(connection).await;
    });
}
