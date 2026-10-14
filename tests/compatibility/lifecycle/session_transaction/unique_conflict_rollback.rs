use super::*;
use cassie::config::CassieRuntimeConfig;
use std::sync::Arc;

#[test]
fn should_wait_for_unique_conflict_owner_rollback_before_returning_insert() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        support::use_local_storage();
        let path = support::data_dir("unique_conflict_owner_rollback");
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
                "CREATE TABLE compat_unique_rollback_rows (id INT PRIMARY KEY, value TEXT UNIQUE)",
            )
            .await
            .expect("create a unique-key transaction fixture");
        execute_statements(
            &owner,
            &[
                "BEGIN",
                "INSERT INTO compat_unique_rollback_rows VALUES (1, 'shared')",
            ],
        )
        .await
        .expect("hold the first unique value in an open transaction");
        contender
            .batch_execute("BEGIN")
            .await
            .expect("begin the competing transaction");

        let mut contender_insert = tokio::spawn(async move {
            let result = contender
                .query_one(
                    "INSERT INTO compat_unique_rollback_rows VALUES (2, 'shared') RETURNING id",
                    &[],
                )
                .await;
            (contender, result)
        });

        // Act
        // Confirm the conflicting request entered Cassie before observing whether it returns.
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
        let returned_before_owner_rollback = early_completion.is_some();
        assert!(
            request_entered_engine || returned_before_owner_rollback,
            "the competing query must reach the engine or complete before owner rollback"
        );

        owner
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back the transaction that first held the unique value");
        let (contender, insert_result) = match early_completion {
            Some(result) => result.expect("competing insert task should complete"),
            None => contender_insert
                .await
                .expect("competing insert task should complete after owner rollback"),
        };
        let committed = contender.batch_execute("COMMIT").await;
        let stored = owner
            .query(
                "SELECT id FROM compat_unique_rollback_rows ORDER BY id",
                &[],
            )
            .await;
        let insert_outcome = match &insert_result {
            Ok(row) => format!("Ok({})", row.get::<_, i32>(0)),
            Err(error) => format!("Err({error})"),
        };

        // Assert
        drop((owner, contender));
        contender_connection.abort();
        let _ = contender_connection.await;
        owner_connection.abort();
        let _ = owner_connection.await;
        server.abort();
        let _ = server.await;
        assert!(
            !returned_before_owner_rollback,
            "PostgreSQL waits for the unresolved unique-key owner before returning INSERT; \
             engine entry observed: {request_entered_engine}; early result: {insert_outcome}"
        );
        let inserted = insert_result.expect("owner rollback makes the competing insert succeed");
        assert_eq!(inserted.get::<_, i32>(0), 2);
        committed.expect("COMMIT after the owner rollback should succeed");
        let stored = stored.expect("read rows after the owner rollback");
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].get::<_, i32>(0), 2);
    });
}
