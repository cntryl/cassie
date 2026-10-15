use super::*;

#[test]
fn should_bind_values_after_formatting_quoted_dynamic_identifiers() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("do_format_using_result_shape").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_836_format_using",
                "CREATE TABLE compat_836_format_using.\"Quoted\"\"Target\" \
                   (id INTEGER PRIMARY KEY, \"Bound\"\"Value\" TEXT)",
                "CREATE TABLE compat_836_format_using.observed \
                   (id INTEGER, observed_value TEXT)",
                "INSERT INTO compat_836_format_using.\"Quoted\"\"Target\" \
                   VALUES (1, 'payload $1; O''Reilly'), (2, NULL)",
            ],
        )
        .await
        .expect("create quoted dynamic SQL objects with nontrivial and NULL values");

        // Act
        client
            .batch_execute(
                r#"DO $block$
                   DECLARE
                     target_schema TEXT := 'compat_836_format_using';
                     target_table TEXT := 'Quoted"Target';
                     target_column TEXT := 'Bound"Value';
                     target_id INTEGER;
                     selected_value TEXT;
                   BEGIN
                     target_id := 1;
                     EXECUTE format(
                       'SELECT %I FROM %I.%I WHERE id = $1',
                       target_column, target_schema, target_table
                     ) INTO selected_value USING target_id;
                     INSERT INTO compat_836_format_using.observed
                       VALUES (target_id, selected_value);

                     target_id := 2;
                     EXECUTE format(
                       'SELECT %I FROM %I.%I WHERE id = $1',
                       target_column, target_schema, target_table
                     ) INTO selected_value USING target_id;
                     INSERT INTO compat_836_format_using.observed
                       VALUES (target_id, selected_value);
                   END;
                   $block$"#,
            )
            .await
            .expect("bind values to formatted, quoted dynamic identifiers and capture results");
        let rows = client
            .query(
                "SELECT id, observed_value, observed_value IS NULL AS value_is_null \
                 FROM compat_836_format_using.observed ORDER BY id",
                &[],
            )
            .await
            .expect("read the dynamic INTO results and NULL indicator");

        // Assert
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].columns()[0].type_().oid(), 23);
        assert_eq!(rows[0].columns()[1].type_().oid(), 25);
        assert_eq!(rows[0].columns()[2].type_().oid(), 16);
        assert_eq!(
            rows.iter()
                .map(|row| (
                    row.get::<_, i32>(0),
                    row.get::<_, Option<String>>(1),
                    row.get::<_, bool>(2),
                ))
                .collect::<Vec<_>>(),
            vec![
                (1, Some("payload $1; O'Reilly".to_owned()), false),
                (2, None, true),
            ]
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
