use super::*;

#[test]
fn should_return_bound_dynamic_query_rows_from_plpgsql_table_function() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("plpgsql_return_query_execute").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_835_return_query_source \
                   (id INTEGER PRIMARY KEY, label TEXT)",
                "INSERT INTO compat_835_return_query_source \
                   VALUES (1, 'one'), (2, 'two'), (3, NULL)",
                "CREATE FUNCTION compat_835_return_query(minimum_id INTEGER) \
                   RETURNS TABLE(returned_id INTEGER, label TEXT) \
                   LANGUAGE plpgsql AS $function$ \
                   BEGIN \
                     RETURN QUERY EXECUTE \
                       'SELECT id, label FROM compat_835_return_query_source \
                        WHERE id >= $1 ORDER BY id' \
                       USING minimum_id; \
                   END; \
                   $function$",
            ],
        )
        .await
        .expect("create a bound dynamic RETURN QUERY table function");

        // Act
        let rows = client
            .query(
                "SELECT returned_id, label \
                 FROM compat_835_return_query(2) ORDER BY returned_id",
                &[],
            )
            .await
            .expect("return matching rows from dynamic SQL");
        let empty_statement = client
            .prepare(
                "SELECT returned_id, label \
                 FROM compat_835_return_query(99) ORDER BY returned_id",
            )
            .await
            .expect("describe the declared columns for an empty dynamic result");
        let empty_rows = client
            .query(&empty_statement, &[])
            .await
            .expect("execute the empty dynamic RETURN QUERY result");

        // Assert
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].columns()[0].type_().oid(), 23);
        assert_eq!(rows[0].columns()[1].type_().oid(), 25);
        assert_eq!(
            rows.iter()
                .map(|row| (row.get::<_, i32>(0), row.get::<_, Option<String>>(1),))
                .collect::<Vec<_>>(),
            vec![(2, Some("two".to_owned())), (3, None),]
        );
        assert_eq!(empty_statement.columns()[0].type_().oid(), 23);
        assert_eq!(empty_statement.columns()[1].type_().oid(), 25);
        assert!(empty_rows.is_empty());

        drop(client);
        server.shutdown(connection).await;
    });
}
