use std::collections::BTreeMap;
use std::time::Duration;

use cassie::app::{Cassie, ProjectionReplayBatch, ProjectionReplayEvent};
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use cassie::types::Value;
use cassie_test_support::support_pgwire as wire;
use cassie_test_support::support_temp_dirs::IsolatedDataDir;

const TABLE: &str = "mixed_oracle_events";
const POLICY: &str = "mixed_oracle_retention";
const SOURCE: &str = "mixed-oracle-event-stream";
const FIRST_CHECKPOINT: &str = "checkpoint-3";
const SECOND_CHECKPOINT: &str = "checkpoint-5";
const PAGE_SIZE: usize = 2;
const RETENTION_CUTOFF: &str = "2026-01-04T00:00:00Z";
const PAGE_LIMIT_SQL: &str = "2026-01-05T00:00:00Z";

#[derive(Clone, Debug)]
struct EventRow {
    id: i64,
    device: String,
    event_at: String,
    amount: i64,
}

#[derive(Clone)]
struct EventMutation {
    event_id: String,
    checkpoint: String,
    position: u64,
    document_id: String,
    row: EventRow,
}

struct OracleEvidence {
    initial_snapshot: Vec<Vec<Vec<Option<String>>>>,
    retained_snapshot: Vec<Vec<Vec<Option<String>>>>,
    replayed_pages: Vec<Vec<Vec<Option<String>>>>,
    post_cutoff_pages: Vec<Vec<Vec<Option<String>>>>,
    reopened_pages: Vec<Vec<Vec<Option<String>>>>,
    state: OracleStateEvidence,
    reopened_state: OracleStateEvidence,
    first_applied: u64,
    second_applied: u64,
    due_command: String,
}

struct OracleStateEvidence {
    relation: Vec<Vec<Value>>,
    rollup: Vec<Vec<Value>>,
    source_rollup: Vec<Vec<Value>>,
    checkpoint: Vec<Vec<Value>>,
    retention_progress: Vec<Vec<Value>>,
}

struct ProjectionSnapshots {
    server: wire::PgwireServer,
    client: PreparedPageClient,
    initial_snapshot: Vec<Vec<Vec<Option<String>>>>,
    retained_snapshot: Vec<Vec<Vec<Option<String>>>>,
    replayed_pages: Vec<Vec<Vec<Option<String>>>>,
    first_applied: u64,
    second_applied: u64,
}

#[test]
fn should_match_independent_event_oracle_across_checkpoint_snapshot_cutoff_and_reopen() {
    // Arrange
    let directory = IsolatedDataDir::new("mixed-workload-oracle");
    let initial_events = initial_events();
    let correction_events = correction_events();

    // Act
    let evidence = runtime().block_on(run_oracle_scenario(
        directory.path(),
        &initial_events,
        &correction_events,
    ));

    // Assert
    let mut model = BTreeMap::new();
    apply_events(&mut model, &initial_events);
    assert_eq!(evidence.first_applied, 3);
    let initial_pages = expected_pages(&model, FIRST_CHECKPOINT);
    assert_eq!(evidence.initial_snapshot, initial_pages);
    assert_eq!(evidence.retained_snapshot, initial_pages);

    apply_events(&mut model, &correction_events);
    assert_eq!(evidence.second_applied, 2);
    let replayed_pages = expected_pages(&model, SECOND_CHECKPOINT);
    assert_eq!(evidence.replayed_pages, replayed_pages);

    model.retain(|_, row| row.event_at.as_str() >= "2026-01-03T00:00:00Z");
    let retained_pages = expected_pages(&model, SECOND_CHECKPOINT);
    assert_eq!(evidence.due_command, "ENFORCE RETENTION 3");
    assert_eq!(evidence.post_cutoff_pages, retained_pages);
    assert_eq!(evidence.reopened_pages, retained_pages);
    assert_eq!(evidence.state.relation, expected_relation(&model));
    assert_eq!(evidence.reopened_state.relation, expected_relation(&model));
    assert_eq!(evidence.reopened_state.relation, evidence.state.relation);
    assert_eq!(evidence.state.rollup, evidence.state.source_rollup);
    assert_rollup_matches_oracle(&evidence.state.rollup, &model);
    assert_eq!(
        evidence.reopened_state.rollup,
        evidence.reopened_state.source_rollup
    );
    assert_rollup_matches_oracle(&evidence.reopened_state.rollup, &model);
    assert_eq!(
        evidence.state.checkpoint,
        vec![vec![Value::String(SECOND_CHECKPOINT.to_owned())]]
    );
    assert_eq!(
        evidence.reopened_state.checkpoint,
        evidence.state.checkpoint
    );
    assert_eq!(
        evidence.state.retention_progress,
        vec![vec![
            Value::Null,
            Value::String("2026-01-04 00:00:00+00".to_owned()),
            Value::Int64(0),
            Value::Int64(3),
            Value::String("2026-01-05 00:00:00+00".to_owned()),
        ]]
    );
    assert_eq!(
        evidence.reopened_state.retention_progress,
        evidence.state.retention_progress
    );
}

async fn run_oracle_scenario(
    data_dir: &std::path::Path,
    initial_events: &[EventMutation],
    correction_events: &[EventMutation],
) -> OracleEvidence {
    let cassie = new_local_cassie(data_dir);
    cassie.startup().expect("start local oracle fixture");
    let session = cassie.create_session("root", None);
    prepare_schema(&cassie, &session);
    let projection = cassie_test_support::support_sql::canonical_test_collection(&cassie, TABLE);
    let mut model = BTreeMap::new();
    apply_events(&mut model, initial_events);
    let mut snapshots =
        project_and_snapshot(&cassie, &projection, initial_events, correction_events).await;
    apply_events(&mut model, correction_events);
    prepare_retention(&cassie, &session);
    let due_command = cassie
        .execute_sql(
            &session,
            &format!("ENFORCE RETENTION POLICY {POLICY} DUE AT '{RETENTION_CUTOFF}'"),
            vec![],
        )
        .expect("complete scheduled retention cutoff")
        .command()
        .to_owned();
    model.retain(|_, row| row.event_at.as_str() >= "2026-01-03T00:00:00Z");
    let post_cutoff_pages = snapshots.client.pages().await;
    let state = read_oracle_state(&cassie, &session, &projection);
    let ProjectionSnapshots {
        server,
        client,
        initial_snapshot,
        retained_snapshot,
        replayed_pages,
        first_applied,
        second_applied,
    } = snapshots;
    client.close().await;
    server.stop().await;
    drop(session);
    drop(cassie);
    let (reopened_pages, reopened_state) = read_reopened_oracle(data_dir, &projection).await;

    OracleEvidence {
        initial_snapshot,
        retained_snapshot,
        replayed_pages,
        post_cutoff_pages,
        reopened_pages,
        state,
        reopened_state,
        first_applied,
        second_applied,
        due_command,
    }
}

async fn project_and_snapshot(
    cassie: &Cassie,
    projection: &str,
    initial_events: &[EventMutation],
    correction_events: &[EventMutation],
) -> ProjectionSnapshots {
    let first = cassie
        .replay_projection_batch_strict(replay_batch(projection, "mixed-batch-3", initial_events))
        .expect("commit initial projected events and checkpoint together");
    let server = wire::spawn_server(cassie.clone()).await;
    let mut client = PreparedPageClient::connect(&server).await;
    client.prepare().await;
    client.begin_repeatable_read().await;
    let initial_snapshot = client.pages().await;

    let second = cassie
        .replay_projection_batch_strict(replay_batch(
            projection,
            "mixed-batch-5",
            correction_events,
        ))
        .expect("commit correction, insertion, and checkpoint together");
    let retained_snapshot = client.pages().await;
    client.commit().await;
    let replayed_pages = client.pages().await;

    ProjectionSnapshots {
        server,
        client,
        initial_snapshot,
        retained_snapshot,
        replayed_pages,
        first_applied: first.applied_event_count,
        second_applied: second.applied_event_count,
    }
}

async fn read_reopened_oracle(
    data_dir: &std::path::Path,
    projection: &str,
) -> (Vec<Vec<Vec<Option<String>>>>, OracleStateEvidence) {
    let reopened = new_local_cassie(data_dir);
    reopened.startup().expect("reopen local oracle fixture");
    let session = reopened.create_session("root", None);
    let server = wire::spawn_server(reopened.clone()).await;
    let mut client = PreparedPageClient::connect(&server).await;
    client.prepare().await;
    let pages = client.pages().await;
    let state = read_oracle_state(&reopened, &session, projection);
    client.close().await;
    server.stop().await;
    drop(session);
    drop(reopened);
    (pages, state)
}

fn read_oracle_state(
    cassie: &Cassie,
    session: &cassie::app::CassieSession,
    projection: &str,
) -> OracleStateEvidence {
    let retention_progress = query_rows(
        cassie,
        session,
        &format!(
            "SELECT pending_cutoff::TEXT,last_successful_cutoff::TEXT,pending_scanned_rows,last_deleted_rows,next_due_at::TEXT FROM pg_catalog.pg_retention_policies WHERE policy_name = '{}'",
            canonical_relation_name(POLICY)
        ),
    );
    let relation = query_rows(
        cassie,
        session,
        &format!("SELECT id,device,event_at::TEXT,amount FROM {TABLE} ORDER BY event_at,id"),
    );
    let rollup_sql = format!(
        "SELECT time_bucket('1 day',event_at) AS bucket,device,COUNT(*) AS total,SUM(amount) AS amount_sum FROM {TABLE} GROUP BY time_bucket('1 day',event_at),device ORDER BY bucket,device"
    );
    let source_rollup_sql = format!(
        "SELECT time_bucket('1 day',event_at) AS bucket,device,COUNT(*) AS total,SUM(amount) AS amount_sum FROM {TABLE} WHERE id >= 0 GROUP BY time_bucket('1 day',event_at),device ORDER BY bucket,device"
    );
    let rollup = query_rows(cassie, session, &rollup_sql);
    let source_rollup = query_rows(cassie, session, &source_rollup_sql);
    let checkpoint = query_rows(
        cassie,
        session,
        &format!(
            "SELECT source_checkpoint FROM pg_catalog.pg_projection_checkpoints WHERE collection = '{projection}'"
        ),
    );

    OracleStateEvidence {
        relation,
        rollup,
        source_rollup,
        checkpoint,
        retention_progress,
    }
}

struct PreparedPageClient {
    reader: tokio::net::tcp::OwnedReadHalf,
    writer: tokio::net::tcp::OwnedWriteHalf,
    next_portal_id: usize,
}

impl PreparedPageClient {
    async fn connect(server: &wire::PgwireServer) -> Self {
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect mixed workload pgwire fixture");
        let (mut reader, mut writer) = socket.into_split();
        wire::complete_startup_with_password(
            &mut reader,
            &mut writer,
            cassie_test_support::support_sql::test_root_password(),
        )
        .await;
        Self {
            reader,
            writer,
            next_portal_id: 0,
        }
    }

    async fn prepare(&mut self) {
        for device in ["alpha", "beta"] {
            let statement = statement_name(device);
            let query = page_sql(device);
            let frames = exchange(
                &mut self.reader,
                &mut self.writer,
                vec![
                    wire::parse_frame(&statement, &query),
                    wire::describe_statement_frame(&statement),
                    wire::sync_frame(),
                ],
            )
            .await;
            assert_no_wire_error(&frames, "prepare ordered page");
            assert_ready_status(&frames, b'I');
        }
    }

    async fn begin_repeatable_read(&mut self) {
        let frames = exchange(
            &mut self.reader,
            &mut self.writer,
            vec![wire::simple_query_frame(
                "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY",
            )],
        )
        .await;
        assert_no_wire_error(&frames, "begin repeatable-read snapshot");
        assert_ready_status(&frames, b'T');
    }

    async fn pages(&mut self) -> Vec<Vec<Vec<Option<String>>>> {
        let mut pages = Vec::new();
        let portal_id = self.next_portal_id;
        self.next_portal_id = self
            .next_portal_id
            .checked_add(1)
            .expect("prepared page portal id does not overflow");
        for device in ["alpha", "beta"] {
            let statement = statement_name(device);
            let portal = format!("mixed_page_{device}_{portal_id}");
            let frames = exchange(
                &mut self.reader,
                &mut self.writer,
                vec![
                    wire::bind_frame(&portal, &statement, &[]),
                    wire::describe_portal_frame(&portal),
                    wire::execute_frame(&portal),
                    wire::sync_frame(),
                ],
            )
            .await;
            assert_no_wire_error(&frames, "execute ordered prepared page");
            let rows = wire::data_rows(&frames);
            assert!(rows.len() <= PAGE_SIZE, "page exceeded its prepared limit");
            pages.push(rows);
        }
        pages
    }

    async fn commit(&mut self) {
        let frames = exchange(
            &mut self.reader,
            &mut self.writer,
            vec![wire::simple_query_frame("COMMIT")],
        )
        .await;
        assert_no_wire_error(&frames, "commit repeatable-read snapshot");
        assert_ready_status(&frames, b'I');
    }

    async fn close(mut self) {
        wire::write_frames(&mut self.writer, vec![vec![b'X', 0, 0, 0, 4]]).await;
        let mut byte = [0];
        let read = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::io::AsyncReadExt::read(&mut self.reader, &mut byte),
        )
        .await
        .expect("terminate connection cleanup deadline")
        .expect("read after terminate");
        assert_eq!(read, 0, "backend must finish connection cleanup");
    }
}

async fn exchange(
    reader: &mut tokio::net::tcp::OwnedReadHalf,
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    frames: Vec<Vec<u8>>,
) -> Vec<(u8, Vec<u8>)> {
    wire::write_frames(writer, frames).await;
    wire::read_frames_until_ready_within(reader, Duration::from_secs(5)).await
}

fn assert_no_wire_error(frames: &[(u8, Vec<u8>)], context: &str) {
    assert_eq!(wire::error_code(frames), None, "{context}: {frames:?}");
}

fn assert_ready_status(frames: &[(u8, Vec<u8>)], expected: u8) {
    assert_eq!(
        frames
            .last()
            .map(|(tag, payload)| (*tag, payload.as_slice())),
        Some((b'Z', &[expected][..]))
    );
}

fn statement_name(device: &str) -> String {
    format!("mixed_page_{device}")
}

fn page_sql(device: &str) -> String {
    format!(
        "SELECT events.id,events.device,events.event_at::TEXT,events.amount,checkpoint.source_checkpoint FROM {TABLE} events CROSS JOIN (SELECT source_checkpoint FROM pg_catalog.pg_projection_checkpoints WHERE collection = '{}') checkpoint WHERE events.device = '{device}' AND events.event_at >= '2026-01-01T00:00:00Z' AND events.event_at < '{PAGE_LIMIT_SQL}' ORDER BY events.event_at,events.id LIMIT {PAGE_SIZE}",
        canonical_relation_name(TABLE)
    )
}

fn prepare_schema(cassie: &Cassie, session: &cassie::app::CassieSession) {
    for sql in [
        format!(
            "CREATE TABLE {TABLE} (id INT PRIMARY KEY, device TEXT NOT NULL, event_at TIMESTAMP NOT NULL, amount INT NOT NULL)"
        ),
        format!(
            "CREATE INDEX mixed_oracle_time ON {TABLE} USING time_series (event_at) WITH (bucket_width = '1 day', partition_by = device)"
        ),
        format!(
            "CREATE ROLLUP mixed_oracle_daily ON {TABLE} USING time_bucket('1 day',event_at) GROUP BY device AGGREGATES COUNT(*) AS total, SUM(amount) AS amount_sum"
        ),
    ] {
        cassie
            .execute_sql(session, &sql, vec![])
            .expect("prepare mixed workload schema");
    }
}

fn prepare_retention(cassie: &Cassie, session: &cassie::app::CassieSession) {
    cassie
        .execute_sql(
            session,
            &format!(
                "CREATE RETENTION POLICY {POLICY} ON {TABLE} USING event_at RETAIN FOR '1 day'"
            ),
            vec![],
        )
        .expect("create retention policy");
    cassie
        .execute_sql(
            session,
            &format!(
                "ALTER RETENTION POLICY {POLICY} SCHEDULE DAILY START AT '{RETENTION_CUTOFF}'"
            ),
            vec![],
        )
        .expect("schedule retention cutoff");
}

fn new_local_cassie(data_dir: &std::path::Path) -> Cassie {
    let mut config =
        cassie_test_support::support_sql::with_test_root_password(CassieRuntimeConfig::default());
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    Cassie::new_local_with_data_dir_and_config(data_dir, config)
        .expect("create explicit local Midge fixture")
}

fn replay_batch(
    projection: &str,
    batch_id: &str,
    mutations: &[EventMutation],
) -> ProjectionReplayBatch {
    ProjectionReplayBatch {
        projection: projection.to_owned(),
        source_identity: SOURCE.to_owned(),
        batch_id: batch_id.to_owned(),
        lag: 0,
        events: mutations
            .iter()
            .map(|mutation| ProjectionReplayEvent {
                event_id: mutation.event_id.clone(),
                checkpoint: mutation.checkpoint.clone(),
                position: Some(mutation.position),
                document_id: mutation.document_id.clone(),
                payload: Some(serde_json::json!({
                    "id": mutation.row.id,
                    "device": mutation.row.device,
                    "event_at": mutation.row.event_at,
                    "amount": mutation.row.amount,
                })),
            })
            .collect(),
    }
}

fn apply_events(model: &mut BTreeMap<i64, EventRow>, mutations: &[EventMutation]) {
    for mutation in mutations {
        model.insert(mutation.row.id, mutation.row.clone());
    }
}

fn expected_pages(
    model: &BTreeMap<i64, EventRow>,
    checkpoint: &str,
) -> Vec<Vec<Vec<Option<String>>>> {
    ["alpha", "beta"]
        .into_iter()
        .map(|device| {
            let mut rows = model
                .values()
                .filter(|row| row.device == device)
                .collect::<Vec<_>>();
            rows.sort_by(|left, right| {
                left.event_at
                    .cmp(&right.event_at)
                    .then_with(|| left.id.cmp(&right.id))
            });
            rows.into_iter()
                .take(PAGE_SIZE)
                .map(|row| {
                    vec![
                        Some(row.id.to_string()),
                        Some(row.device.clone()),
                        Some(expected_timestamp_text(&row.event_at)),
                        Some(row.amount.to_string()),
                        Some(checkpoint.to_owned()),
                    ]
                })
                .collect()
        })
        .collect()
}

fn expected_relation(model: &BTreeMap<i64, EventRow>) -> Vec<Vec<Value>> {
    let mut rows = model.values().collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        left.event_at
            .cmp(&right.event_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    rows.into_iter()
        .map(|row| {
            vec![
                Value::Int64(row.id),
                Value::String(row.device.clone()),
                Value::String(expected_timestamp_text(&row.event_at)),
                Value::Int64(row.amount),
            ]
        })
        .collect()
}

fn assert_rollup_matches_oracle(rows: &[Vec<Value>], model: &BTreeMap<i64, EventRow>) {
    let mut expected = BTreeMap::<(String, String), (i64, i64)>::new();
    for row in model.values() {
        let day = row.event_at[..10].to_owned();
        let entry = expected.entry((day, row.device.clone())).or_default();
        entry.0 += 1;
        entry.1 += row.amount;
    }
    assert_eq!(rows.len(), expected.len());
    for (row, ((day, device), (count, amount_sum))) in rows.iter().zip(expected) {
        assert!(matches!(&row[0], Value::String(bucket) if bucket.starts_with(&day)));
        assert_eq!(row[1], Value::String(device));
        assert_eq!(row[2], Value::Int64(count));
        assert_eq!(row[3], Value::Int64(amount_sum));
    }
}

fn query_rows(cassie: &Cassie, session: &cassie::app::CassieSession, sql: &str) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(session, sql, vec![])
        .expect("read oracle evidence")
        .rows()
        .to_vec()
}

fn expected_timestamp_text(value: &str) -> String {
    value
        .strip_suffix('Z')
        .expect("oracle event timestamps use explicit UTC")
        .replace('T', " ")
}

fn canonical_relation_name(name: &str) -> String {
    cassie::catalog::canonical_relation_name("postgres", "public", name)
}

fn initial_events() -> Vec<EventMutation> {
    vec![
        event(
            "event-1",
            1,
            1,
            "checkpoint-1",
            "alpha",
            "2026-01-01T01:00:00Z",
            10,
        ),
        event(
            "event-2",
            2,
            2,
            "checkpoint-2",
            "alpha",
            "2026-01-02T00:00:00Z",
            20,
        ),
        event(
            "event-3",
            3,
            3,
            FIRST_CHECKPOINT,
            "beta",
            "2026-01-02T12:00:00Z",
            5,
        ),
    ]
}

fn correction_events() -> Vec<EventMutation> {
    vec![
        // Source positions advance after checkpoint-3 while event time moves backward.
        event(
            "event-4",
            4,
            3,
            "checkpoint-4",
            "beta",
            "2026-01-01T12:00:00Z",
            7,
        ),
        event(
            "event-5",
            5,
            4,
            SECOND_CHECKPOINT,
            "beta",
            "2026-01-04T03:00:00Z",
            11,
        ),
    ]
}

fn event(
    event_id: &str,
    position: u64,
    document_id: i64,
    checkpoint: &str,
    device: &str,
    event_at: &str,
    amount: i64,
) -> EventMutation {
    EventMutation {
        event_id: event_id.to_owned(),
        checkpoint: checkpoint.to_owned(),
        position,
        document_id: document_id.to_string(),
        row: EventRow {
            id: document_id,
            device: device.to_owned(),
            event_at: event_at.to_owned(),
            amount,
        },
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime")
}
