use super::*;

const PARTITION_PARENT: &str = "compat840_finalize_root";
const PARTITION_CHILD: &str = "compat840_finalize_child";

async fn relation_row_count(
    client: &tokio_postgres::Client,
    relation: &str,
) -> Result<i64, tokio_postgres::Error> {
    let query = format!("SELECT count(*) FROM {relation}");
    let row = client.query_one(&query, &[]).await?;
    Ok(row.get(0))
}

async fn detach_snapshot(
    client: &tokio_postgres::Client,
) -> Result<(Option<bool>, i64, i64), tokio_postgres::Error> {
    let pending_query = format!(
        "SELECT inhdetachpending FROM pg_catalog.pg_inherits \
         WHERE inhparent = '{PARTITION_PARENT}'::regclass \
           AND inhrelid = '{PARTITION_CHILD}'::regclass"
    );
    let pending = client
        .query_opt(&pending_query, &[])
        .await?
        .map(|row| row.get::<_, bool>(0));
    let parent_count = relation_row_count(client, PARTITION_PARENT).await?;
    let child_count = relation_row_count(client, PARTITION_CHILD).await?;
    Ok((pending, parent_count, child_count))
}

async fn detached_partition_check(
    client: &tokio_postgres::Client,
) -> Result<Option<(String, bool)>, tokio_postgres::Error> {
    let query = format!(
        "SELECT pg_catalog.pg_get_constraintdef(constraint_row.oid), \
                constraint_row.convalidated \
         FROM pg_catalog.pg_constraint AS constraint_row \
         WHERE constraint_row.conrelid = '{PARTITION_CHILD}'::regclass \
           AND constraint_row.contype = 'c'"
    );
    let row = client.query_opt(&query, &[]).await?;
    Ok(row.map(|row| (row.get::<_, String>(0), row.get::<_, bool>(1))))
}

#[test]
fn should_finalize_interrupted_concurrent_detach_after_lock_timeout() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("issue840_detach_finalize_pending").await;
        let (locker, locker_connection) = server.connect().await;
        let (detacher, detacher_connection) = server.connect().await;
        execute_statements(
            &detacher,
            &[
                "CREATE TABLE compat840_finalize_root (id INT) \
                 PARTITION BY RANGE (id)",
                "CREATE TABLE compat840_finalize_child PARTITION OF \
                 compat840_finalize_root FOR VALUES FROM (0) TO (10)",
                "INSERT INTO compat840_finalize_root VALUES (1), (2)",
            ],
        )
        .await
        .expect("create and populate one bounded range partition");
        locker
            .batch_execute("BEGIN")
            .await
            .expect("begin a transaction that keeps using the partitioned parent");
        locker
            .query_one("SELECT count(*) FROM compat840_finalize_root", &[])
            .await
            .expect("read the partitioned parent while keeping the transaction open");
        detacher
            .batch_execute("SET lock_timeout = '500ms'")
            .await
            .expect("set a bounded wait for the concurrent detach");

        // Act
        let detach_result = detacher
            .batch_execute(
                "ALTER TABLE compat840_finalize_root DETACH PARTITION \
                 compat840_finalize_child CONCURRENTLY",
            )
            .await;
        let detach_sqlstate = detach_result
            .as_ref()
            .err()
            .map(|error| db_error(error).code().code().to_owned());
        let pending_before_finalize = detach_snapshot(&detacher).await;
        let release_reader = locker.batch_execute("COMMIT").await;
        let finalize_result = detacher
            .batch_execute(
                "ALTER TABLE compat840_finalize_root DETACH PARTITION \
                 compat840_finalize_child FINALIZE",
            )
            .await;
        let pending_after_finalize = detach_snapshot(&detacher).await;
        let partition_check = detached_partition_check(&detacher).await;

        // Assert
        drop(locker);
        drop(detacher);
        locker_connection.abort();
        let _ = locker_connection.await;
        server.shutdown(detacher_connection).await;
        assert_eq!(detach_sqlstate.as_deref(), Some("55P03"));
        assert_eq!(
            pending_before_finalize.expect("read the state after lock timeout"),
            (Some(true), 0, 2)
        );
        assert!(
            release_reader.is_ok(),
            "the transaction holding the parent read should finish: {release_reader:?}"
        );
        assert!(
            finalize_result.is_ok(),
            "FINALIZE should complete the interrupted concurrent detach: {finalize_result:?}"
        );
        assert_eq!(
            pending_after_finalize.expect("read the state after finalization"),
            (None, 0, 2)
        );
        assert_eq!(
            partition_check.expect("read the detached partition constraint"),
            Some((
                "CHECK (((id IS NOT NULL) AND (id >= 0) AND (id < 10)))".to_owned(),
                true,
            ))
        );
    });
}
