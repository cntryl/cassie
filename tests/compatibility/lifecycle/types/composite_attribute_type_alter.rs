use super::*;

#[test]
fn should_cascade_composite_attribute_type_changes_to_typed_table_rows() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("composite_attribute_type_cascade_828").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE SCHEMA compat_composite_attr_alter_828",
                "CREATE TYPE compat_composite_attr_alter_828.measurement \
                 AS (count SMALLINT, label TEXT)",
                "CREATE TABLE compat_composite_attr_alter_828.measurements \
                 OF compat_composite_attr_alter_828.measurement",
                "INSERT INTO compat_composite_attr_alter_828.measurements \
                 VALUES (32767, 'preserved')",
            ],
        )
        .await
        .expect("create a typed table with a stored composite row");

        // Act
        client
            .batch_execute(
                "ALTER TYPE compat_composite_attr_alter_828.measurement \
                 ALTER ATTRIBUTE count SET DATA TYPE BIGINT CASCADE",
            )
            .await
            .expect("cascade an attribute type change to its typed table");
        let rows = client
            .query(
                "SELECT count, label \
                 FROM compat_composite_attr_alter_828.measurements",
                &[],
            )
            .await
            .expect("read the converted stored composite row");
        let attribute_type: i64 = client
            .query_one(
                "SELECT atttypid::BIGINT \
                 FROM pg_catalog.pg_attribute \
                 WHERE attrelid = \
                   'compat_composite_attr_alter_828.measurements'::regclass \
                   AND attname = 'count' AND attnum > 0 AND NOT attisdropped",
                &[],
            )
            .await
            .expect("read the typed-table attribute identity")
            .get(0);

        // Assert
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].columns()[0].type_().oid(), 20);
        assert_eq!(rows[0].get::<_, i64>(0), 32_767);
        assert_eq!(rows[0].get::<_, String>(1), "preserved");
        assert_eq!(attribute_type, 20);

        drop(client);
        server.shutdown(connection).await;
    });
}
