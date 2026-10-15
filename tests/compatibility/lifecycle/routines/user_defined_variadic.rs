use super::*;

#[test]
fn should_preserve_user_defined_variadic_array_semantics() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("user_defined_variadic_routine").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_user_variadic",
                "CREATE FUNCTION compat_user_variadic.count_items(VARIADIC items TEXT[]) \
                   RETURNS INT LANGUAGE SQL IMMUTABLE AS $$ SELECT cardinality(items) $$",
            ],
        )
        .await
        .expect("create a SQL routine with a declared VARIADIC array argument");

        // Act
        let row = client
            .query_one(
                "SELECT compat_user_variadic.count_items('north', 'star') AS expanded, \
                        compat_user_variadic.count_items( \
                          VARIADIC ARRAY['north', 'star']::TEXT[] \
                        ) AS packed, \
                        compat_user_variadic.count_items( \
                          VARIADIC ARRAY[]::TEXT[] \
                        ) AS empty_array, \
                        compat_user_variadic.count_items(NULL::TEXT) AS null_element, \
                        compat_user_variadic.count_items( \
                          VARIADIC NULL::TEXT[] \
                        ) AS null_array",
                &[],
            )
            .await
            .expect("execute expanded and array-form calls to the user-defined variadic routine");

        // Assert
        assert!(row
            .columns()
            .iter()
            .all(|column| column.type_().oid() == 23));
        assert_eq!(row.get::<_, i32>(0), 2);
        assert_eq!(row.get::<_, i32>(1), 2);
        assert_eq!(row.get::<_, i32>(2), 0);
        assert_eq!(row.get::<_, i32>(3), 1);
        assert_eq!(row.get::<_, Option<i32>>(4), None);

        drop(client);
        server.shutdown(connection).await;
    });
}
