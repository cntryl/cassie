#[test]
fn should_isolate_temporary_views_to_the_owning_session() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("temporary_view_isolation").await;
        let (owner, owner_connection) = server.connect().await;
        let (observer, observer_connection) = server.connect().await;
        owner
            .batch_execute("CREATE TABLE compat_temp_view_source (id INT)")
            .await
            .expect("create a permanent source table for the temporary view");
        owner
            .batch_execute("INSERT INTO compat_temp_view_source VALUES (1)")
            .await
            .expect("seed the temporary-view source table");
        owner
            .batch_execute(
                "CREATE TEMP VIEW compat_temp_session_view AS \
                 SELECT id FROM compat_temp_view_source",
            )
            .await
            .expect("create a session-local view over the permanent source table");

        // Act
        let visible = owner
            .query_one("SELECT id FROM compat_temp_session_view", &[])
            .await
            .expect("read the view in the owning session");
        let invisible = observer
            .query_one("SELECT id FROM compat_temp_session_view", &[])
            .await
            .expect_err("another session must not resolve a temporary view");

        // Assert
        assert_eq!(visible.get::<_, i32>(0), 1);
        assert_eq!(db_error(&invisible).code().code(), "42P01");

        drop((owner, observer));
        server.shutdown(owner_connection).await;
        observer_connection.abort();
        let _ = observer_connection.await;
    });
}
