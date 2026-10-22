use super::*;
use cassie::config::CassieRuntimeConfig;
use std::sync::Arc;

type SelectRecheckTask = tokio::task::JoinHandle<(
    tokio_postgres::Client,
    Result<Vec<tokio_postgres::Row>, tokio_postgres::Error>,
)>;

struct SelectRecheckFixture {
    cassie: Cassie,
    server: tokio::task::JoinHandle<()>,
    owner: tokio_postgres::Client,
    owner_connection: tokio::task::JoinHandle<()>,
    worker_connection: tokio::task::JoinHandle<()>,
    worker_select: SelectRecheckTask,
}

struct SelectRecheckObservations {
    request_entered_engine: bool,
    completed_before_owner_commit: bool,
    returned_rows: Vec<tokio_postgres::Row>,
    final_state: String,
}

async fn create_select_recheck_fixture() -> SelectRecheckFixture {
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
            "CREATE TABLE compat_lock_select_recheck_rows \
               (id INT PRIMARY KEY, state TEXT NOT NULL); \
             INSERT INTO compat_lock_select_recheck_rows VALUES (1, 'ready'); \
             BEGIN; \
             SELECT id FROM compat_lock_select_recheck_rows \
               WHERE id = 1 FOR UPDATE",
        )
        .await
        .expect("hold the matching row in an owner transaction");
    let worker_select = tokio::spawn(async move {
        let result = worker
            .query(
                "SELECT id, state FROM compat_lock_select_recheck_rows \
                 WHERE state = 'ready' FOR UPDATE",
                &[],
            )
            .await;
        (worker, result)
    });
    SelectRecheckFixture {
        cassie,
        server,
        owner,
        owner_connection,
        worker_connection,
        worker_select,
    }
}

async fn observe_select_recheck(
    cassie: &Cassie,
    owner: &tokio_postgres::Client,
    mut worker_select: SelectRecheckTask,
) -> (tokio_postgres::Client, SelectRecheckObservations) {
    let request_entered_engine = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if worker_select.is_finished() {
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
    let early_completion = tokio::time::timeout(Duration::from_millis(500), &mut worker_select)
        .await
        .ok();
    let completed_before_owner_commit = early_completion.is_some();
    owner
        .batch_execute(
            "UPDATE compat_lock_select_recheck_rows \
             SET state = 'closed' WHERE id = 1; COMMIT",
        )
        .await
        .expect("change the predicate row and release the owner lock");
    let (worker, select_result) = match early_completion {
        Some(result) => result.expect("competing SELECT task should complete"),
        None => worker_select
            .await
            .expect("competing SELECT should complete after owner commit"),
    };
    let returned_rows = select_result.expect("recheck the SELECT FOR UPDATE predicate");
    let final_state = owner
        .query_one(
            "SELECT state FROM compat_lock_select_recheck_rows WHERE id = 1",
            &[],
        )
        .await
        .expect("read the row after both transactions finish")
        .get(0);
    (
        worker,
        SelectRecheckObservations {
            request_entered_engine,
            completed_before_owner_commit,
            returned_rows,
            final_state,
        },
    )
}

fn assert_select_recheck(observations: &SelectRecheckObservations) {
    assert!(
        observations.request_entered_engine || observations.completed_before_owner_commit,
        "the competing SELECT must reach the engine or complete before owner resolution"
    );
    assert!(
        !observations.completed_before_owner_commit,
        "PostgreSQL waits for the row-lock owner before returning SELECT FOR UPDATE"
    );
    assert!(
        observations.returned_rows.is_empty(),
        "PostgreSQL rechecks the SELECT predicate after the owner changes the row"
    );
    assert_eq!(observations.final_state, "closed");
}

#[test]
fn should_recheck_select_for_update_predicate_after_concurrent_update_commits() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let fixture = create_select_recheck_fixture().await;
        let SelectRecheckFixture {
            cassie,
            server,
            owner,
            owner_connection,
            worker_connection,
            worker_select,
        } = fixture;

        // Act
        let (worker, observations) = observe_select_recheck(&cassie, &owner, worker_select).await;

        // Assert
        drop((owner, worker));
        worker_connection.abort();
        let _ = worker_connection.await;
        owner_connection.abort();
        let _ = owner_connection.await;
        server.abort();
        let _ = server.await;
        assert_select_recheck(&observations);
    });
}
