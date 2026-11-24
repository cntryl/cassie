use super::*;

#[test]
fn should_accept_cursor_sensitivity_options_with_ordered_results() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("cursor_sensitivity_options").await;
        let (client, connection) = server.connect().await;
        client
            .execute("CREATE TABLE compat_cursor_sensitivity_values (n INT)", &[])
            .await
            .expect("create ordered cursor input rows");
        client
            .execute(
                "INSERT INTO compat_cursor_sensitivity_values VALUES (1), (2), (3), (4)",
                &[],
            )
            .await
            .expect("insert ordered cursor input rows");
        client
            .batch_execute(
                "BEGIN; \
                 DECLARE compat_asensitive ASENSITIVE NO SCROLL CURSOR FOR \
                   SELECT n FROM compat_cursor_sensitivity_values WHERE n <= 2 ORDER BY n",
            )
            .await
            .expect("declare an ASENSITIVE NO SCROLL cursor");

        // Act
        let asensitive_metadata = client
            .query_one(
                "SELECT is_scrollable, is_holdable, is_binary FROM pg_cursors \
                 WHERE name = 'compat_asensitive'",
                &[],
            )
            .await
            .expect("inspect the ASENSITIVE cursor descriptor");
        let asensitive_rows = client
            .query("FETCH ALL FROM compat_asensitive", &[])
            .await
            .expect("fetch rows from the ASENSITIVE cursor")
            .iter()
            .map(|row| row.get::<_, i32>(0))
            .collect::<Vec<_>>();
        client
            .batch_execute("CLOSE compat_asensitive")
            .await
            .expect("close the ASENSITIVE cursor");
        client
            .batch_execute(
                "DECLARE compat_insensitive INSENSITIVE SCROLL CURSOR FOR \
                 SELECT n FROM compat_cursor_sensitivity_values WHERE n >= 3 ORDER BY n",
            )
            .await
            .expect("declare an INSENSITIVE SCROLL cursor");
        let insensitive_metadata = client
            .query_one(
                "SELECT is_scrollable, is_holdable, is_binary FROM pg_cursors \
                 WHERE name = 'compat_insensitive'",
                &[],
            )
            .await
            .expect("inspect the INSENSITIVE cursor descriptor");
        let insensitive_rows = client
            .query("FETCH ALL FROM compat_insensitive", &[])
            .await
            .expect("fetch rows from the INSENSITIVE cursor")
            .iter()
            .map(|row| row.get::<_, i32>(0))
            .collect::<Vec<_>>();
        client
            .batch_execute("CLOSE compat_insensitive; COMMIT")
            .await
            .expect("close the INSENSITIVE cursor and commit");

        // Assert
        assert_eq!(
            asensitive_metadata
                .columns()
                .iter()
                .map(|column| column.type_().oid())
                .collect::<Vec<_>>(),
            vec![16, 16, 16]
        );
        assert_eq!(
            (
                asensitive_metadata.get::<_, bool>(0),
                asensitive_metadata.get::<_, bool>(1),
                asensitive_metadata.get::<_, bool>(2),
            ),
            (false, false, false)
        );
        assert_eq!(asensitive_rows, vec![1, 2]);
        assert_eq!(
            (
                insensitive_metadata.get::<_, bool>(0),
                insensitive_metadata.get::<_, bool>(1),
                insensitive_metadata.get::<_, bool>(2),
            ),
            (true, false, false)
        );
        assert_eq!(insensitive_rows, vec![3, 4]);

        drop(client);
        server.shutdown(connection).await;
    });
}
