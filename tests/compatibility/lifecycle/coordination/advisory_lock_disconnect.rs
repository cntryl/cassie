use super::*;

#[test]
fn should_release_session_advisory_lock_when_owner_disconnects() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("advisory_lock_disconnect").await;
        let (owner, owner_connection) = server.connect().await;
        let (contender, contender_connection) = server.connect().await;
        let owner_lock = owner
            .query_one("SELECT pg_advisory_lock(91822009)", &[])
            .await;
        let held_result: Option<Result<bool, tokio_postgres::Error>> = if owner_lock.is_ok() {
            Some(
                contender
                    .query_one("SELECT pg_try_advisory_lock(91822009)", &[])
                    .await
                    .and_then(|row| row.try_get(0)),
            )
        } else {
            None
        };

        // Act
        drop(owner);
        let owner_closed = tokio::time::timeout(Duration::from_secs(2), owner_connection).await;
        let owner_finished = owner_closed.as_ref().is_ok_and(Result::is_ok);
        let available_result: Option<Result<bool, tokio_postgres::Error>> =
            if owner_finished && held_result.as_ref().is_some_and(Result::is_ok) {
                Some(
                    contender
                        .query_one("SELECT pg_try_advisory_lock(91822009)", &[])
                        .await
                        .and_then(|row| row.try_get(0)),
                )
            } else {
                None
            };
        let released_result: Option<Result<bool, tokio_postgres::Error>> = if available_result
            .as_ref()
            .is_some_and(|result| matches!(result, Ok(true)))
        {
            Some(
                contender
                    .query_one("SELECT pg_advisory_unlock(91822009)", &[])
                    .await
                    .and_then(|row| row.try_get(0)),
            )
        } else {
            None
        };

        drop(contender);
        server.shutdown(contender_connection).await;

        // Assert
        owner_lock.expect("owner should acquire a session advisory lock");
        assert!(owner_finished, "the owning connection should close");
        let held = held_result
            .expect("contender should observe the held lock")
            .expect("try-lock result");
        let available = available_result
            .expect("contender should acquire after owner disconnect")
            .expect("try-lock result");
        let released = released_result
            .expect("contender should release the acquired lock")
            .expect("unlock result");
        assert!(!held, "session advisory locks cannot be successful no-ops");
        assert!(
            available,
            "disconnect must release the owner's session lock"
        );
        assert!(released);
    });
}
