use super::*;
use cassie::config::CassieRuntimeConfig;
use std::sync::Arc;

#[test]
fn should_wait_for_unique_conflict_owner_commit_before_returning_unique_violation() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        support::use_local_storage();
        let path = support::data_dir("unique_conflict_owner_commit");
        let _directory = DataDirectory(path.clone());
        let mut server_config = CassieRuntimeConfig::from_env().expect("runtime config");
        server_config.password = "postgres".to_string();
        let cassie = Cassie::new_with_data_dir_and_config(&path, server_config.clone())
            .expect("create instrumented Cassie instance");
        cassie
            .startup()
            .expect("start instrumented Cassie instance");
        let server_app = Arc::new(cassie.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind pgwire listener");
        let addr = listener.local_addr().expect("listener address");
        drop(listener);
        let server = tokio::spawn(async move {
            let _ = cassie::pgwire::server::run(addr.to_string(), server_app, server_config).await;
        });
        tokio::time::sleep(Duration::from_millis(50)).await;

        let mut connection_config = tokio_postgres::Config::new();
        connection_config
            .host("127.0.0.1")
            .port(addr.port())
            .user("root")
            .password("postgres")
            .dbname("postgres");
        let (owner, owner_connection) = connection_config
            .connect(NoTls)
            .await
            .expect("connect unique-key owner");
        let owner_connection = tokio::spawn(async move {
            owner_connection
                .await
                .expect("owner pgwire connection should stay healthy");
        });
        let (contender, contender_connection) = connection_config
            .connect(NoTls)
            .await
            .expect("connect competing session");
        let contender_connection = tokio::spawn(async move {
            contender_connection
                .await
                .expect("competing pgwire connection should stay healthy");
        });
        owner
            .batch_execute(
                "CREATE TABLE compat_unique_commit_rows (id INT PRIMARY KEY, value TEXT UNIQUE)",
            )
            .await
            .expect("create a unique-key transaction fixture");
        owner
            .batch_execute("BEGIN")
            .await
            .expect("begin the first transaction");
        let owner_insert = owner
            .query_one(
                "INSERT INTO compat_unique_commit_rows VALUES (1, 'shared') RETURNING id",
                &[],
            )
            .await
            .expect("hold the first unique value in an open transaction");
        assert_eq!(owner_insert.get::<_, i32>(0), 1);
        contender
            .batch_execute("BEGIN")
            .await
            .expect("begin the competing transaction");

        let mut contender_insert = tokio::spawn(async move {
            let result = contender
                .query_one(
                    "INSERT INTO compat_unique_commit_rows VALUES (2, 'shared') RETURNING id",
                    &[],
                )
                .await;
            (contender, result)
        });

        // Act
        // Observe engine entry before measuring whether INSERT ... RETURNING completes.
        let request_entered_engine = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if contender_insert.is_finished() {
                    return false;
                }
                if cassie.metrics()["runtime"]["running_queries"]
                    .as_u64()
                    .is_some_and(|running| running > 0)
                {
                    return true;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap_or(false);
        let early_completion =
            tokio::time::timeout(Duration::from_millis(500), &mut contender_insert)
                .await
                .ok();
        let completed_before_owner_commit = early_completion.is_some();

        owner
            .batch_execute("COMMIT")
            .await
            .expect("commit the transaction that first held the unique value");
        let (contender, insert_result) = match early_completion {
            Some(result) => result.expect("competing insert task should complete"),
            None => contender_insert
                .await
                .expect("competing insert should complete after owner commit"),
        };
        let insert_error_code = insert_result
            .as_ref()
            .err()
            .and_then(|error| error.as_db_error())
            .map(|error| error.code().code().to_string());
        let returned_id = insert_result
            .as_ref()
            .ok()
            .map(|row| row.get::<_, i32>(0));
        let contender_commit = contender.batch_execute("COMMIT").await;
        if contender_commit.is_err() {
            let _ = contender.batch_execute("ROLLBACK").await;
        }
        let committed_rows = owner
            .query(
                "SELECT id FROM compat_unique_commit_rows ORDER BY id",
                &[],
            )
            .await;

        // Assert
        drop((owner, contender));
        contender_connection.abort();
        let _ = contender_connection.await;
        owner_connection.abort();
        let _ = owner_connection.await;
        server.abort();
        let _ = server.await;
        assert!(
            request_entered_engine || completed_before_owner_commit,
            "the conflicting INSERT must reach the engine or complete before owner commit"
        );
        assert!(
            !completed_before_owner_commit,
            "PostgreSQL waits for the unresolved unique-key owner before returning INSERT ... RETURNING"
        );
        assert_eq!(
            insert_error_code.as_deref(),
            Some("23505"),
            "the contender INSERT itself must report unique_violation after the owner commits; returned row: {returned_id:?}"
        );
        assert!(
            contender_commit.is_ok(),
            "COMMIT after a failed INSERT must finish as ROLLBACK, not report the conflict at commit: {contender_commit:?}"
        );
        let committed_rows = committed_rows.expect("read rows after both transactions finish");
        assert_eq!(committed_rows.len(), 1);
        assert_eq!(committed_rows[0].get::<_, i32>(0), 1);
    });
}
