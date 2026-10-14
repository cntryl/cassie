use super::*;
use cassie::config::CassieRuntimeConfig;
use std::sync::Arc;

#[test]
fn should_recheck_update_predicate_after_a_row_lock_wait() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        support::use_local_storage();
        let path = support::data_dir("row_lock_predicate_recheck");
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
        let addr = listener.local_addr().expect("pgwire listener address");
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
            .expect("connect row-lock owner");
        let owner_connection = tokio::spawn(async move {
            owner_connection
                .await
                .expect("owner pgwire connection should stay healthy");
        });
        let (worker, worker_connection) = connection_config
            .connect(NoTls)
            .await
            .expect("connect competing row-lock worker");
        let worker_connection = tokio::spawn(async move {
            worker_connection
                .await
                .expect("worker pgwire connection should stay healthy");
        });
        owner
            .batch_execute(
                "CREATE TABLE compat_lock_recheck_rows (id INT PRIMARY KEY, state TEXT NOT NULL)",
            )
            .await
            .expect("create the row-lock recheck fixture");
        owner
            .batch_execute(
                "INSERT INTO compat_lock_recheck_rows VALUES (1, 'ready'); \
                 BEGIN; SELECT id FROM compat_lock_recheck_rows WHERE id = 1 FOR UPDATE",
            )
            .await
            .expect("hold the matching row in an owner transaction");
        let mut worker_update = tokio::spawn(async move {
            let result = worker
                .query(
                    "UPDATE compat_lock_recheck_rows SET state = 'claimed' \
                     WHERE id = 1 AND state = 'ready' RETURNING id",
                    &[],
                )
                .await;
            (worker, result)
        });

        // Act
        let request_entered_engine = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if worker_update.is_finished() {
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
        let early_completion = tokio::time::timeout(Duration::from_millis(500), &mut worker_update)
            .await
            .ok();
        let completed_before_owner_commit = early_completion.is_some();
        owner
            .batch_execute(
                "UPDATE compat_lock_recheck_rows SET state = 'closed' WHERE id = 1; COMMIT",
            )
            .await
            .expect("change the row predicate and release the owner lock");
        let (worker, update_result) = match early_completion {
            Some(result) => result.expect("competing UPDATE task should complete"),
            None => worker_update
                .await
                .expect("competing UPDATE should complete after owner commit"),
        };
        let returned_rows = update_result.expect("recheck the competing UPDATE predicate");
        let final_state: String = owner
            .query_one(
                "SELECT state FROM compat_lock_recheck_rows WHERE id = 1",
                &[],
            )
            .await
            .expect("read the row after both transactions finish")
            .try_get(0)
            .expect("final row state");

        // Assert
        drop((owner, worker));
        worker_connection.abort();
        let _ = worker_connection.await;
        owner_connection.abort();
        let _ = owner_connection.await;
        server.abort();
        let _ = server.await;
        assert!(
            request_entered_engine || completed_before_owner_commit,
            "the competing UPDATE must reach the engine or complete before owner resolution"
        );
        assert!(
            !completed_before_owner_commit,
            "PostgreSQL waits for the row-lock owner before returning the UPDATE"
        );
        assert!(
            returned_rows.is_empty(),
            "PostgreSQL rechecks the WHERE predicate after the owner changes the row"
        );
        assert_eq!(final_state, "closed");
    });
}

#[test]
fn should_allow_no_key_update_while_another_session_holds_key_share() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("row_lock_strength_compatibility").await;
        let (owner, owner_connection) = server.connect().await;
        let (worker, worker_connection) = server.connect().await;
        execute_statements(
            &owner,
            &[
                "CREATE TABLE compat_lock_strength_rows (id INT PRIMARY KEY, value INT)",
                "INSERT INTO compat_lock_strength_rows VALUES (1, 10)",
                "BEGIN",
            ],
        )
        .await
        .expect("create a row and acquire a transaction owner");
        let owner_row: i32 = owner
            .query_one(
                "SELECT id FROM compat_lock_strength_rows WHERE id = 1 FOR KEY SHARE",
                &[],
            )
            .await
            .expect("acquire a key-share row lock")
            .try_get(0)
            .expect("owner row id");
        worker
            .batch_execute("BEGIN")
            .await
            .expect("begin the independent lock transaction");

        // Act
        let worker_row: i32 = worker
            .query_one(
                "SELECT id FROM compat_lock_strength_rows WHERE id = 1 \
                 FOR NO KEY UPDATE NOWAIT",
                &[],
            )
            .await
            .expect("NO KEY UPDATE is compatible with the held KEY SHARE lock")
            .try_get(0)
            .expect("worker row id");
        owner
            .batch_execute("COMMIT")
            .await
            .expect("commit the key-share owner");
        worker
            .batch_execute("COMMIT")
            .await
            .expect("commit the no-key-update worker");

        // Assert
        assert_eq!(owner_row, 1);
        assert_eq!(worker_row, 1);

        drop((owner, worker));
        server.shutdown(owner_connection).await;
        worker_connection.abort();
        let _ = worker_connection.await;
    });
}

#[test]
fn should_lock_only_the_relation_named_by_for_update_of() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("row_lock_of_scope").await;
        let (owner, owner_connection) = server.connect().await;
        let (worker, worker_connection) = server.connect().await;
        execute_statements(
            &owner,
            &[
                "CREATE TABLE compat_lock_of_left (id INT PRIMARY KEY)",
                "CREATE TABLE compat_lock_of_right (id INT PRIMARY KEY)",
                "INSERT INTO compat_lock_of_left VALUES (1)",
                "INSERT INTO compat_lock_of_right VALUES (1)",
                "BEGIN",
            ],
        )
        .await
        .expect("create the joined row-lock fixture");
        let alias_control: i32 = owner
            .query_one(
                "SELECT l.id FROM compat_lock_of_left AS l \
                 JOIN compat_lock_of_right AS r ON l.id = r.id \
                 WHERE l.id = 1",
                &[],
            )
            .await
            .expect("resolve the same aliases and predicate without a locking clause")
            .try_get(0)
            .expect("control row id");
        let owner_row: i32 = owner
            .query_one(
                "SELECT l.id FROM compat_lock_of_left AS l \
                 JOIN compat_lock_of_right AS r ON l.id = r.id \
                 WHERE l.id = 1 \
                 FOR UPDATE OF l",
                &[],
            )
            .await
            .expect("lock only the explicitly named relation")
            .try_get(0)
            .expect("owner row id");
        worker
            .batch_execute("BEGIN")
            .await
            .expect("begin the independent lock transaction");

        // Act
        let unlocked_relation_row = worker
            .query_one(
                "SELECT id FROM compat_lock_of_right WHERE id = 1 FOR UPDATE NOWAIT",
                &[],
            )
            .await
            .expect("the joined relation omitted from OF should remain unlocked");
        let selected_relation_conflict = worker
            .query_one(
                "SELECT id FROM compat_lock_of_left WHERE id = 1 FOR UPDATE NOWAIT",
                &[],
            )
            .await
            .expect_err("the relation named by OF should hold the row lock");
        owner
            .batch_execute("ROLLBACK")
            .await
            .expect("release the owner's row lock");
        worker
            .batch_execute("ROLLBACK")
            .await
            .expect("release the worker transaction after the lock conflict");

        // Assert
        assert_eq!(alias_control, 1);
        assert_eq!(owner_row, 1);
        assert_eq!(unlocked_relation_row.get::<_, i32>(0), 1);
        assert_eq!(db_error(&selected_relation_conflict).code().code(), "55P03");

        drop((owner, worker));
        server.shutdown(owner_connection).await;
        worker_connection.abort();
        let _ = worker_connection.await;
    });
}
