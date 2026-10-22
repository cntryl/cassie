use super::*;

type RowLockRecheckTask = tokio::task::JoinHandle<(
    tokio_postgres::Client,
    Result<Vec<tokio_postgres::Row>, tokio_postgres::Error>,
)>;

struct RowLockRecheckFixture {
    cassie: Cassie,
    server: tokio::task::JoinHandle<()>,
    owner: tokio_postgres::Client,
    owner_connection: tokio::task::JoinHandle<()>,
    worker_connection: tokio::task::JoinHandle<()>,
    worker_update: RowLockRecheckTask,
}

struct RowLockRecheckObservations {
    request_entered_engine: bool,
    completed_before_owner_commit: bool,
    returned_rows: Vec<tokio_postgres::Row>,
    final_state: String,
}

async fn create_row_lock_recheck_fixture() -> RowLockRecheckFixture {
    let mut server_config = CassieRuntimeConfig::from_env().expect("runtime config");
    server_config.password = "postgres".to_string();
    let cassie = Cassie::new_in_memory_with_config(server_config.clone())
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
    let worker_update = tokio::spawn(async move {
        let result = worker
            .query(
                "UPDATE compat_lock_recheck_rows SET state = 'claimed' \
                 WHERE id = 1 AND state = 'ready' RETURNING id",
                &[],
            )
            .await;
        (worker, result)
    });
    RowLockRecheckFixture {
        cassie,
        server,
        owner,
        owner_connection,
        worker_connection,
        worker_update,
    }
}

async fn observe_row_lock_recheck(
    cassie: &Cassie,
    owner: &tokio_postgres::Client,
    mut worker_update: RowLockRecheckTask,
) -> (tokio_postgres::Client, RowLockRecheckObservations) {
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
        .batch_execute("UPDATE compat_lock_recheck_rows SET state = 'closed' WHERE id = 1; COMMIT")
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

    (
        worker,
        RowLockRecheckObservations {
            request_entered_engine,
            completed_before_owner_commit,
            returned_rows,
            final_state,
        },
    )
}

fn assert_row_lock_recheck_observations(observations: &RowLockRecheckObservations) {
    assert!(
        observations.request_entered_engine || observations.completed_before_owner_commit,
        "the competing UPDATE must reach the engine or complete before owner resolution"
    );
    assert!(
        !observations.completed_before_owner_commit,
        "PostgreSQL waits for the row-lock owner before returning the UPDATE"
    );
    assert!(
        observations.returned_rows.is_empty(),
        "PostgreSQL rechecks the WHERE predicate after the owner changes the row"
    );
    assert_eq!(observations.final_state, "closed");
}
use cassie::config::CassieRuntimeConfig;
use std::sync::Arc;

fn assert_row_lock_pair(
    server_name: &'static str,
    owner_mode: &'static str,
    contender_mode: &'static str,
    expected_conflict: bool,
) {
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start(server_name).await;
        let (owner, owner_connection) = server.connect().await;
        let (contender, contender_connection) = server.connect().await;
        execute_statements(
            &owner,
            &[
                "CREATE TABLE compat_remaining_lock_pairs (id INT PRIMARY KEY)",
                "INSERT INTO compat_remaining_lock_pairs VALUES (1)",
            ],
        )
        .await
        .expect("create a single row for the lock-pair check");
        contender
            .batch_execute("SET statement_timeout = '2s'")
            .await
            .expect("bound a missing NOWAIT implementation");
        owner
            .batch_execute("BEGIN")
            .await
            .expect("begin the lock-owner transaction");
        let owner_query =
            format!("SELECT id FROM compat_remaining_lock_pairs WHERE id = 1 FOR {owner_mode}");
        let owner_row = owner
            .query_one(&owner_query, &[])
            .await
            .expect("acquire the first row lock mode");
        contender
            .batch_execute("BEGIN")
            .await
            .expect("begin the competing lock transaction");
        let contender_query = format!(
            "SELECT id FROM compat_remaining_lock_pairs WHERE id = 1 \
             FOR {contender_mode} NOWAIT"
        );

        // Act
        let contender_result = contender.query_one(&contender_query, &[]).await;

        // Assert
        assert_eq!(owner_row.get::<_, i32>(0), 1);
        if expected_conflict {
            let conflict = contender_result.expect_err("NOWAIT must reject a conflicting row lock");
            assert_eq!(
                db_error(&conflict).code().code(),
                "55P03",
                "wrong SQLSTATE for {owner_mode} versus {contender_mode}: {conflict}"
            );
            let aborted_followup = contender.query_one("SELECT 1", &[]).await;
            assert_eq!(
                db_error(&aborted_followup.expect_err("a failed lock aborts its transaction"))
                    .code()
                    .code(),
                "25P02"
            );
            contender
                .batch_execute("ROLLBACK")
                .await
                .expect("recover the contender after NOWAIT failed");
            owner
                .batch_execute("COMMIT")
                .await
                .expect("release the conflicting row lock");
            let retry = contender
                .query_one(&contender_query, &[])
                .await
                .expect("the contender acquires the row after release");
            assert_eq!(retry.get::<_, i32>(0), 1);
        } else {
            let contender_row = contender_result
                .expect("PostgreSQL permits compatible row-lock modes")
                .get::<_, i32>(0);
            assert_eq!(contender_row, 1);
            owner
                .batch_execute("COMMIT")
                .await
                .expect("release the compatible owner lock");
            contender
                .batch_execute("COMMIT")
                .await
                .expect("release the compatible contender lock");
        }

        drop((owner, contender));
        server.shutdown(owner_connection).await;
        contender_connection.abort();
        let _ = contender_connection.await;
    });
}

#[test]
fn should_recheck_update_predicate_after_a_row_lock_wait() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let fixture = create_row_lock_recheck_fixture().await;
        let RowLockRecheckFixture {
            cassie,
            server,
            owner,
            owner_connection,
            worker_connection,
            worker_update,
        } = fixture;

        // Act
        let (worker, observations) = observe_row_lock_recheck(&cassie, &owner, worker_update).await;

        // Assert
        drop((owner, worker));
        worker_connection.abort();
        let _ = worker_connection.await;
        owner_connection.abort();
        let _ = owner_connection.await;
        server.abort();
        let _ = server.await;
        assert_row_lock_recheck_observations(&observations);
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

#[test]
fn should_conflict_no_key_update_with_a_held_share_lock() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("row_lock_no_key_update_share_conflict").await;
        let (owner, owner_connection) = server.connect().await;
        let (contender, contender_connection) = server.connect().await;
        execute_statements(
            &owner,
            &[
                "CREATE TABLE compat_no_key_share_conflict (id INT PRIMARY KEY)",
                "INSERT INTO compat_no_key_share_conflict VALUES (1)",
                "BEGIN",
            ],
        )
        .await
        .expect("create the row and begin the lock owner transaction");
        let held_row: i32 = owner
            .query_one(
                "SELECT id FROM compat_no_key_share_conflict WHERE id = 1 FOR SHARE",
                &[],
            )
            .await
            .expect("hold a SHARE lock on the fixture row")
            .get(0);
        contender
            .batch_execute("BEGIN")
            .await
            .expect("begin the competing row-lock transaction");

        // Act
        let conflict = contender
            .query(
                "SELECT id FROM compat_no_key_share_conflict \
                 WHERE id = 1 FOR NO KEY UPDATE NOWAIT",
                &[],
            )
            .await
            .expect_err("NO KEY UPDATE NOWAIT conflicts with another session's SHARE lock");
        let aborted_followup = contender.query_one("SELECT 1", &[]).await;
        contender
            .batch_execute("ROLLBACK")
            .await
            .expect("recover the contender after its failed lock statement");
        owner
            .batch_execute("COMMIT")
            .await
            .expect("release the SHARE lock");
        let retry: i32 = contender
            .query_one(
                "SELECT id FROM compat_no_key_share_conflict \
                 WHERE id = 1 FOR NO KEY UPDATE NOWAIT",
                &[],
            )
            .await
            .expect("acquire NO KEY UPDATE after the conflicting lock is released")
            .get(0);

        // Assert
        assert_eq!(held_row, 1);
        assert_eq!(db_error(&conflict).code().code(), "55P03");
        assert_eq!(
            db_error(
                &aborted_followup.expect_err("the lock error aborts the explicit transaction")
            )
            .code()
            .code(),
            "25P02"
        );
        assert_eq!(retry, 1);

        drop((owner, contender));
        server.shutdown(owner_connection).await;
        contender_connection.abort();
        let _ = contender_connection.await;
    });
}

#[test]
fn should_allow_key_share_with_key_share() {
    // Arrange
    let owner_mode = "KEY SHARE";
    let contender_mode = "KEY SHARE";

    // Act
    assert_row_lock_pair("row_lock_key_share_pair", owner_mode, contender_mode, false);

    // Assert
    // The helper checks that both sessions return row id 1 without a lock error.
}

#[test]
fn should_allow_key_share_with_share() {
    // Arrange
    let owner_mode = "KEY SHARE";
    let contender_mode = "SHARE";

    // Act
    assert_row_lock_pair(
        "row_lock_key_share_share_pair",
        owner_mode,
        contender_mode,
        false,
    );

    // Assert
    // The helper checks that both sessions return row id 1 without a lock error.
}

#[test]
fn should_allow_share_with_share() {
    // Arrange
    let owner_mode = "SHARE";
    let contender_mode = "SHARE";

    // Act
    assert_row_lock_pair("row_lock_share_pair", owner_mode, contender_mode, false);

    // Assert
    // The helper checks that both sessions return row id 1 without a lock error.
}

#[test]
fn should_conflict_no_key_update_with_no_key_update() {
    // Arrange
    let owner_mode = "NO KEY UPDATE";
    let contender_mode = "NO KEY UPDATE";

    // Act
    assert_row_lock_pair(
        "row_lock_no_key_update_pair",
        owner_mode,
        contender_mode,
        true,
    );

    // Assert
    // The helper checks SQLSTATE 55P03, transaction recovery, and lock release.
}

#[test]
fn should_conflict_update_with_no_key_update() {
    // Arrange
    let owner_mode = "NO KEY UPDATE";
    let contender_mode = "UPDATE";

    // Act
    assert_row_lock_pair(
        "row_lock_update_no_key_update_pair",
        owner_mode,
        contender_mode,
        true,
    );

    // Assert
    // The helper checks SQLSTATE 55P03, transaction recovery, and lock release.
}
