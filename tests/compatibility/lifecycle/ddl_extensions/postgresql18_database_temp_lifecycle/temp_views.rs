use super::*;

#[test]
fn should_isolate_temporary_views_between_sessions() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("temporary_view_session_isolation").await;
        let (owner, owner_connection) = server.connect().await;
        let (observer, observer_connection) = server.connect().await;

        // Act
        owner
            .batch_execute("CREATE TEMP VIEW compat_temp_view AS SELECT 18 AS id")
            .await
            .expect("create a session-local temporary view");
        let invisible = observer
            .query("SELECT id FROM compat_temp_view", &[])
            .await
            .expect_err("a second session cannot resolve the owner's temporary view");
        observer
            .batch_execute("CREATE TEMP VIEW compat_temp_view AS SELECT 19 AS id")
            .await
            .expect("allow another session to create a view with the same temporary name");
        let owner_row = owner
            .query_one("SELECT id FROM compat_temp_view", &[])
            .await
            .expect("read the owner's temporary view");
        let observer_row = observer
            .query_one("SELECT id FROM compat_temp_view", &[])
            .await
            .expect("read the observer's independent temporary view");

        // Assert
        assert_eq!(db_error(&invisible).code().code(), "42P01");
        assert_eq!(owner_row.get::<_, i32>(0), 18);
        assert_eq!(observer_row.get::<_, i32>(0), 19);

        drop((owner, observer));
        server.shutdown(owner_connection).await;
        observer_connection.abort();
        let _ = observer_connection.await;
    });
}
