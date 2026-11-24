use super::*;

#[test]
fn should_reset_selected_connection_state_with_discard_all() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("discard_all_selected_state").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE TABLE compat_discard_rows (id INT PRIMARY KEY)")
            .await
            .expect("create the persistent cursor relation");
        client
            .batch_execute("INSERT INTO compat_discard_rows VALUES (1), (2)")
            .await
            .expect("seed the persistent cursor relation");
        client
            .batch_execute("SET application_name = 'discard-all-probe'")
            .await
            .expect("set the session parameter to be reset");
        client
            .batch_execute("PREPARE compat_discard_prepared (INT) AS SELECT $1 + 1")
            .await
            .expect("prepare SQL state to be discarded");
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin the holdable cursor transaction");
        client
            .batch_execute(
                "DECLARE compat_discard_cursor CURSOR WITH HOLD FOR \
                 SELECT id FROM compat_discard_rows ORDER BY id",
            )
            .await
            .expect("declare a holdable cursor over the persistent rows");
        client
            .batch_execute("COMMIT")
            .await
            .expect("keep the holdable cursor after its transaction commits");

        // Act
        client
            .batch_execute("DISCARD ALL")
            .await
            .expect("discard selected resettable session state");
        let application_name: String = client
            .query_one("SHOW application_name", &[])
            .await
            .expect("read application_name after DISCARD ALL")
            .get(0);
        let prepared = client
            .query_one("EXECUTE compat_discard_prepared(41)", &[])
            .await;
        let cursor = client
            .query("FETCH FORWARD 1 FROM compat_discard_cursor", &[])
            .await;

        // Assert
        assert_eq!(application_name, "");
        assert_eq!(
            db_error(&prepared.expect_err("DISCARD ALL deallocates SQL PREPARE state"))
                .code()
                .code(),
            "26000"
        );
        assert_eq!(
            db_error(&cursor.expect_err("DISCARD ALL closes open cursors"))
                .code()
                .code(),
            "34000"
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
