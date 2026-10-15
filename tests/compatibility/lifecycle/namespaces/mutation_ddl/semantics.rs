#[test]
fn should_honor_session_local_temporary_table_drop() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("temporary_table_lifecycle").await;
        let (owner, owner_connection) = server.connect().await;
        let (observer, observer_connection) = server.connect().await;
        execute_statements(
            &owner,
            &[
                "CREATE TEMP TABLE compat_temp_rows (id INT) ON COMMIT PRESERVE ROWS",
                "INSERT INTO compat_temp_rows VALUES (7)",
            ],
        )
        .await
        .expect("create session-local temp table");

        // Act
        let private_rows = owner
            .query("SELECT id FROM compat_temp_rows", &[])
            .await
            .expect("read temp table in owner session");
        let invisible = observer
            .query("SELECT id FROM compat_temp_rows", &[])
            .await
            .expect_err("another session cannot see a temp table");
        execute_statements(
            &owner,
            &[
                "BEGIN",
                "CREATE TEMP TABLE compat_temp_drop (id INT) ON COMMIT DROP",
                "INSERT INTO compat_temp_drop VALUES (8)",
                "COMMIT",
            ],
        )
        .await
        .expect("drop ON COMMIT relation at transaction completion");
        let dropped = owner
            .query("SELECT id FROM compat_temp_drop", &[])
            .await
            .expect_err("ON COMMIT DROP table must be absent");

        // Assert
        assert_eq!(private_rows.len(), 1);
        assert_eq!(private_rows[0].get::<_, i32>(0), 7);
        assert_eq!(db_error(&invisible).code().code(), "42P01");
        assert_eq!(db_error(&dropped).code().code(), "42P01");

        drop((owner, observer));
        server.shutdown(owner_connection).await;
        observer_connection.abort();
        let _ = observer_connection.await;
    });
}
