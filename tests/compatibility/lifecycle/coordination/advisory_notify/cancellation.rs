#[test]
fn should_cancel_blocking_advisory_lock_wait_at_statement_timeout() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("advisory_lock_wait_cancellation").await;
        let (owner, owner_connection) = server.connect().await;
        let (contender, contender_connection) = server.connect().await;
        owner
            .query_one("SELECT pg_advisory_lock(91822021)", &[])
            .await
            .expect("hold the session advisory lock");
        contender
            .batch_execute("SET statement_timeout = '50ms'")
            .await
            .expect("set a deadline for the blocking lock request");

        // Act
        let mut lock_wait = tokio::spawn(async move {
            let result = contender
                .query_one("SELECT pg_advisory_lock(91822021)", &[])
                .await;
            (contender, result)
        });
        let bounded_wait = tokio::time::timeout(Duration::from_secs(2), &mut lock_wait).await;
        let owner_released_for_cleanup = bounded_wait.is_err();
        if owner_released_for_cleanup {
            owner
                .query_one("SELECT pg_advisory_unlock(91822021)", &[])
                .await
                .expect("release the owner lock if statement timeout did not fire");
        }
        let (contender, cancelled) = match bounded_wait {
            Ok(joined) => joined.expect("lock wait task should complete"),
            Err(_) => tokio::time::timeout(Duration::from_secs(2), &mut lock_wait)
                .await
                .expect("lock wait should complete after owner releases the lock")
                .expect("lock wait task should complete after owner release"),
        };
        let cancellation_sqlstate = cancelled
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code().to_string());
        let acquired_during_cleanup = if cancelled.is_ok() {
            contender
                .query_one("SELECT pg_advisory_unlock(91822021)", &[])
                .await
                .and_then(|row| row.try_get::<_, bool>(0))
                .unwrap_or(false)
        } else {
            false
        };
        contender
            .batch_execute("RESET statement_timeout")
            .await
            .expect("restore the contender session timeout");
        let recovered: i32 = contender
            .query_one("SELECT 1", &[])
            .await
            .expect("reuse the contender session after cancellation")
            .get(0);
        if !owner_released_for_cleanup {
            owner
                .query_one("SELECT pg_advisory_unlock(91822021)", &[])
                .await
                .expect("release the owner lock after the cancellation result");
        }
        let acquired_after_release: bool = contender
            .query_one("SELECT pg_try_advisory_lock(91822021)", &[])
            .await
            .expect("acquire the released advisory lock")
            .get(0);
        let contender_released: bool = contender
            .query_one("SELECT pg_advisory_unlock(91822021)", &[])
            .await
            .expect("release the contender session lock")
            .get(0);

        // Assert
        assert_eq!(cancellation_sqlstate.as_deref(), Some("57014"));
        assert!(!acquired_during_cleanup);
        assert_eq!(recovered, 1);
        assert!(acquired_after_release);
        assert!(contender_released);

        drop((owner, contender));
        server.shutdown(owner_connection).await;
        contender_connection.abort();
        let _ = contender_connection.await;
    });
}
