use super::*;

const SOURCE_TABLE: &str = "compat_issue833_like_source";
const TARGET_TABLE: &str = "compat_issue833_like_target";
const CTAS_SOURCE_TABLE: &str = "compat_issue833_ctas_source";
const CTAS_TARGET_TABLE: &str = "compat_issue833_ctas_target";

struct StagedCloneObservation {
    sequence_name: String,
    staged_id: i32,
    staged_table_count: i64,
    observer_table_count: i64,
    observer_sequence_count: i64,
    rolled_back_table_count: i64,
    rolled_back_sequence_count: i64,
}

struct CommittedCloneObservation {
    recreated_id: i32,
    committed_table_count: i64,
    committed_sequence_count: i64,
    committed_id: i32,
    committed_label: String,
    next_id: i32,
}

struct CtasSavepointObservation {
    owner_rows: Vec<(i32, String)>,
    staged_table_count: i64,
    observer_table_count: i64,
    rolled_back_table_count: i64,
}

async fn target_table_count(client: &tokio_postgres::Client) -> i64 {
    client
        .query_one(
            "SELECT count(*) FROM pg_class WHERE relname = $1::text",
            &[&TARGET_TABLE],
        )
        .await
        .expect("inspect target relation")
        .get(0)
}

async fn sequence_catalog_count(client: &tokio_postgres::Client, name: &str) -> i64 {
    client
        .query_one(
            "SELECT count(*) FROM pg_class WHERE oid = to_regclass($1::text)",
            &[&name],
        )
        .await
        .expect("inspect target sequence relation")
        .get(0)
}

async fn stage_and_rollback_clone(
    owner: &tokio_postgres::Client,
    observer: &tokio_postgres::Client,
) -> StagedCloneObservation {
    owner
        .batch_execute(&format!(
            "CREATE TABLE {TARGET_TABLE} (LIKE {SOURCE_TABLE} INCLUDING ALL)"
        ))
        .await
        .expect("stage target-owned identity and constraints");
    let sequence_name: String = owner
        .query_one(
            &format!("SELECT pg_get_serial_sequence('public.{TARGET_TABLE}', 'id')"),
            &[],
        )
        .await
        .expect("inspect the target-owned identity sequence")
        .get(0);
    let staged_id: i32 = owner
        .query_one(
            &format!("INSERT INTO {TARGET_TABLE} (label) VALUES ('rolled back') RETURNING id"),
            &[],
        )
        .await
        .expect("insert through the staged identity")
        .get(0);
    let staged_table_count = target_table_count(owner).await;
    let observer_table_count = target_table_count(observer).await;
    let observer_sequence_count = sequence_catalog_count(observer, &sequence_name).await;

    owner
        .batch_execute("ROLLBACK TO SAVEPOINT before_like_clone")
        .await
        .expect("discard target catalog and row effects");
    let rolled_back_table_count = target_table_count(owner).await;
    let rolled_back_sequence_count = sequence_catalog_count(owner, &sequence_name).await;

    StagedCloneObservation {
        sequence_name,
        staged_id,
        staged_table_count,
        observer_table_count,
        observer_sequence_count,
        rolled_back_table_count,
        rolled_back_sequence_count,
    }
}

async fn recreate_and_commit_clone(
    owner: &tokio_postgres::Client,
    observer: &tokio_postgres::Client,
    sequence_name: &str,
) -> CommittedCloneObservation {
    owner
        .batch_execute(&format!(
            "CREATE TABLE {TARGET_TABLE} (LIKE {SOURCE_TABLE} INCLUDING ALL)"
        ))
        .await
        .expect("recreate target name after rollback");
    let recreated_id: i32 = owner
        .query_one(
            &format!("INSERT INTO {TARGET_TABLE} (label) VALUES ('committed') RETURNING id"),
            &[],
        )
        .await
        .expect("use recreated target-owned identity")
        .get(0);
    owner
        .batch_execute("COMMIT")
        .await
        .expect("publish replacement clone");
    let committed_row = observer
        .query_one(&format!("SELECT id, label FROM {TARGET_TABLE}"), &[])
        .await
        .expect("read committed clone and row from the observer");
    let committed_sequence_count = sequence_catalog_count(observer, sequence_name).await;
    let next_id: i32 = observer
        .query_one(
            &format!("INSERT INTO {TARGET_TABLE} (label) VALUES ('next') RETURNING id"),
            &[],
        )
        .await
        .expect("advance committed target-owned identity")
        .get(0);

    CommittedCloneObservation {
        recreated_id,
        committed_table_count: target_table_count(observer).await,
        committed_sequence_count,
        committed_id: committed_row.get(0),
        committed_label: committed_row.get(1),
        next_id,
    }
}

async fn run_like_clone_savepoint_scenario() -> (StagedCloneObservation, CommittedCloneObservation)
{
    let server = CompatibilityServer::start("issue833_like_savepoint").await;
    let (owner, owner_connection) = server.connect().await;
    let (observer, observer_connection) = server.connect().await;
    owner
        .batch_execute(&format!(
            "CREATE TABLE {SOURCE_TABLE} (\
               id INT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY,\
               label TEXT NOT NULL DEFAULT 'template',\
               CONSTRAINT compat_issue833_like_unique_label UNIQUE (label)\
             )"
        ))
        .await
        .expect("create committed source relation");
    owner
        .batch_execute("BEGIN")
        .await
        .expect("begin clone transaction");
    owner
        .batch_execute("SAVEPOINT before_like_clone")
        .await
        .expect("save empty transaction state");

    let staged = stage_and_rollback_clone(&owner, &observer).await;
    let committed = recreate_and_commit_clone(&owner, &observer, &staged.sequence_name).await;

    drop(owner);
    drop(observer);
    observer_connection.abort();
    server.shutdown(owner_connection).await;
    (staged, committed)
}

async fn run_ctas_savepoint_scenario() -> CtasSavepointObservation {
    let server = CompatibilityServer::start("issue833_ctas_savepoint").await;
    let (owner, owner_connection) = server.connect().await;
    let (observer, observer_connection) = server.connect().await;
    owner
        .batch_execute(&format!(
            "CREATE TABLE {CTAS_SOURCE_TABLE} (id INT, label TEXT)"
        ))
        .await
        .expect("create CTAS source");
    owner
        .batch_execute(&format!(
            "INSERT INTO {CTAS_SOURCE_TABLE} VALUES (1, 'one'), (2, 'two')"
        ))
        .await
        .expect("seed CTAS source");
    owner
        .batch_execute("BEGIN")
        .await
        .expect("begin CTAS transaction");
    owner
        .batch_execute("SAVEPOINT before_ctas")
        .await
        .expect("save transaction state before CTAS");
    owner
        .batch_execute(&format!(
            "CREATE TABLE {CTAS_TARGET_TABLE} AS \
             SELECT id, label FROM {CTAS_SOURCE_TABLE} ORDER BY id"
        ))
        .await
        .expect("stage CTAS relation and rows");
    let owner_rows = owner
        .query(
            &format!("SELECT id, label FROM {CTAS_TARGET_TABLE} ORDER BY id"),
            &[],
        )
        .await
        .expect("read staged CTAS rows")
        .iter()
        .map(|row| (row.get::<_, i32>(0), row.get::<_, String>(1)))
        .collect::<Vec<_>>();
    let staged_table_count: i64 = target_ctas_relation_count(&owner, CTAS_TARGET_TABLE).await;
    let observer_table_count: i64 = target_ctas_relation_count(&observer, CTAS_TARGET_TABLE).await;
    owner
        .batch_execute("ROLLBACK TO SAVEPOINT before_ctas")
        .await
        .expect("restore transaction state before CTAS");
    let rolled_back_table_count: i64 = target_ctas_relation_count(&owner, CTAS_TARGET_TABLE).await;
    owner
        .batch_execute("COMMIT")
        .await
        .expect("commit recovered transaction");

    drop(owner);
    drop(observer);
    observer_connection.abort();
    server.shutdown(owner_connection).await;
    CtasSavepointObservation {
        owner_rows,
        staged_table_count,
        observer_table_count,
        rolled_back_table_count,
    }
}

async fn target_ctas_relation_count(client: &tokio_postgres::Client, name: &str) -> i64 {
    client
        .query_one(
            "SELECT count(*) FROM pg_class WHERE relname = $1::text",
            &[&name],
        )
        .await
        .expect("inspect CTAS relation count")
        .get(0)
}

#[test]
fn should_restore_like_identity_publication_to_savepoint() {
    // Arrange
    let runtime = current_thread_runtime();

    // Act
    let (staged, committed) = runtime.block_on(run_like_clone_savepoint_scenario());

    // Assert
    assert_eq!(staged.staged_id, 1);
    assert_eq!(staged.staged_table_count, 1);
    assert_eq!(staged.observer_table_count, 0);
    assert_eq!(staged.observer_sequence_count, 0);
    assert_eq!(staged.rolled_back_table_count, 0);
    assert_eq!(staged.rolled_back_sequence_count, 0);
    assert_eq!(committed.recreated_id, 1);
    assert_eq!(committed.committed_table_count, 1);
    assert_eq!(committed.committed_sequence_count, 1);
    assert_eq!(committed.committed_id, 1);
    assert_eq!(committed.committed_label, "committed");
    assert_eq!(committed.next_id, 2);
}

#[test]
fn should_rollback_ctas_rows_and_catalog_to_savepoint() {
    // Arrange
    let runtime = current_thread_runtime();

    // Act
    let observation = runtime.block_on(run_ctas_savepoint_scenario());

    // Assert
    assert_eq!(
        observation.owner_rows,
        vec![(1, "one".to_owned()), (2, "two".to_owned())]
    );
    assert_eq!(observation.staged_table_count, 1);
    assert_eq!(observation.observer_table_count, 0);
    assert_eq!(observation.rolled_back_table_count, 0);
}
