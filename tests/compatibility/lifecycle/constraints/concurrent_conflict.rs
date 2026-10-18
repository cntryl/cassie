use super::*;
use cassie::config::CassieRuntimeConfig;
use std::sync::Arc;

struct ExclusionConflictFixture {
    _directory: DataDirectory,
    cassie: Cassie,
    server: tokio::task::JoinHandle<()>,
    owner: tokio_postgres::Client,
    owner_connection: tokio::task::JoinHandle<()>,
    contender: tokio_postgres::Client,
    contender_connection: tokio::task::JoinHandle<()>,
}

struct ExclusionConflictObservations {
    request_entered_engine: bool,
    completed_before_owner_commit: bool,
    insert_error_code: Option<String>,
    returned_id: Option<i32>,
    contender_commit: Result<(), tokio_postgres::Error>,
    committed_rows: Result<Vec<tokio_postgres::Row>, tokio_postgres::Error>,
}

async fn create_exclusion_conflict_fixture() -> ExclusionConflictFixture {
    support::use_local_storage();
    let path = support::data_dir("exclusion_conflict_owner_commit");
    let directory = DataDirectory(path.clone());
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
        .expect("connect exclusion-conflict owner");
    let owner_connection = tokio::spawn(async move {
        owner_connection
            .await
            .expect("owner pgwire connection should stay healthy");
    });
    let (contender, contender_connection) = connection_config
        .connect(NoTls)
        .await
        .expect("connect competing exclusion session");
    let contender_connection = tokio::spawn(async move {
        contender_connection
            .await
            .expect("competing pgwire connection should stay healthy");
    });
    owner
        .batch_execute(
            "CREATE TABLE compat_exclusion_concurrent_rows (\
               id INT PRIMARY KEY, slot INT NOT NULL, \
               CONSTRAINT compat_exclusion_concurrent_slot \
                 EXCLUDE USING btree (slot WITH =))",
        )
        .await
        .expect("create an immediate exclusion-constraint fixture");
    owner
        .batch_execute("BEGIN")
        .await
        .expect("begin the transaction that owns the first excluded key");
    let owner_insert = owner
        .query_one(
            "INSERT INTO compat_exclusion_concurrent_rows VALUES (1, 7) RETURNING id",
            &[],
        )
        .await
        .expect("hold the first excluded key in an open transaction");
    assert_eq!(owner_insert.get::<_, i32>(0), 1);
    contender
        .batch_execute("BEGIN")
        .await
        .expect("begin the competing transaction");

    ExclusionConflictFixture {
        _directory: directory,
        cassie,
        server,
        owner,
        owner_connection,
        contender,
        contender_connection,
    }
}

async fn observe_exclusion_conflict(
    cassie: &Cassie,
    owner: &tokio_postgres::Client,
    contender: tokio_postgres::Client,
) -> (tokio_postgres::Client, ExclusionConflictObservations) {
    let mut contender_insert = tokio::spawn(async move {
        let result = contender
            .query_one(
                "INSERT INTO compat_exclusion_concurrent_rows VALUES (2, 7) RETURNING id",
                &[],
            )
            .await;
        (contender, result)
    });
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
    let early_completion = tokio::time::timeout(Duration::from_millis(500), &mut contender_insert)
        .await
        .ok();
    let completed_before_owner_commit = early_completion.is_some();
    owner
        .batch_execute("COMMIT")
        .await
        .expect("commit the transaction that first held the excluded key");
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
    let returned_id = insert_result.as_ref().ok().map(|row| row.get::<_, i32>(0));
    let contender_commit = contender.batch_execute("COMMIT").await;
    if contender_commit.is_err() {
        let _ = contender.batch_execute("ROLLBACK").await;
    }
    let committed_rows = owner
        .query(
            "SELECT id, slot FROM compat_exclusion_concurrent_rows ORDER BY id",
            &[],
        )
        .await;
    (
        contender,
        ExclusionConflictObservations {
            request_entered_engine,
            completed_before_owner_commit,
            insert_error_code,
            returned_id,
            contender_commit,
            committed_rows,
        },
    )
}

fn assert_exclusion_conflict(observations: ExclusionConflictObservations) {
    assert!(
        observations.request_entered_engine || observations.completed_before_owner_commit,
        "the conflicting INSERT must reach the engine or complete before owner commit"
    );
    assert!(
        !observations.completed_before_owner_commit,
        "PostgreSQL waits for the unresolved exclusion-conflict owner before returning INSERT ... RETURNING"
    );
    assert_eq!(
        observations.insert_error_code.as_deref(),
        Some("23P01"),
        "the contender INSERT itself must report exclusion_violation after owner commit; returned row: {:?}",
        observations.returned_id
    );
    observations.contender_commit.expect(
        "COMMIT after the failed INSERT should finish as ROLLBACK rather than report the conflict at commit",
    );
    let committed_rows = observations
        .committed_rows
        .expect("read rows after both transactions finish");
    assert_eq!(committed_rows.len(), 1);
    assert_eq!(committed_rows[0].get::<_, i32>(0), 1);
    assert_eq!(committed_rows[0].get::<_, i32>(1), 7);
}

#[test]
fn should_wait_for_exclusion_conflict_owner_commit_before_returning_violation() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let fixture = create_exclusion_conflict_fixture().await;
        let ExclusionConflictFixture {
            _directory,
            cassie,
            server,
            owner,
            owner_connection,
            contender,
            contender_connection,
        } = fixture;
        // Act
        let (contender, observations) =
            observe_exclusion_conflict(&cassie, &owner, contender).await;

        // Assert
        drop((owner, contender));
        contender_connection.abort();
        let _ = contender_connection.await;
        owner_connection.abort();
        let _ = owner_connection.await;
        server.abort();
        let _ = server.await;
        assert_exclusion_conflict(observations);
    });
}
