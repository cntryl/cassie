//! Current database authority at retained materialized and cursor portal pages.

use crate::support_pgwire as wire;
use crate::support_sql_fixture::{sql_fixture_with_config, SqlFixture};
use cassie::app::{Cassie, CassieSession};
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use std::time::Duration;
use tokio::io::{ReadHalf, WriteHalf};
use tokio::net::TcpStream;

type Frames = Vec<(u8, Vec<u8>)>;

#[derive(Clone, Copy)]
pub enum Scenario {
    MaterializedRevoke,
    MaterializedDrop,
    StreamingRevoke,
}

impl Scenario {
    fn query(self) -> &'static str {
        match self {
            Self::MaterializedRevoke | Self::MaterializedDrop => {
                "SELECT n FROM wire_copy_denied ORDER BY n"
            }
            Self::StreamingRevoke => "SELECT n FROM wire_copy_denied",
        }
    }

    fn mutation(self) -> &'static str {
        match self {
            Self::MaterializedRevoke | Self::StreamingRevoke => {
                "REVOKE CONNECT ON DATABASE postgres FROM boundary_reader"
            }
            Self::MaterializedDrop => "DROP ROLE boundary_reader",
        }
    }
}

struct Client {
    reader: ReadHalf<TcpStream>,
    writer: WriteHalf<TcpStream>,
}

impl Client {
    async fn connect(address: std::net::SocketAddr, user: &str, password: &str) -> Self {
        let socket = TcpStream::connect(address)
            .await
            .expect("network connection");
        let (mut reader, mut writer) = tokio::io::split(socket);
        wire::complete_startup_as(&mut reader, &mut writer, user, "postgres", password).await;
        Self { reader, writer }
    }

    async fn cycle(&mut self, frames: Vec<Vec<u8>>) -> Frames {
        wire::write_frames(&mut self.writer, frames).await;
        wire::read_frames_until_ready_within(&mut self.reader, Duration::from_secs(5)).await
    }
}

pub struct FirstPage {
    frames: Frames,
    before: serde_json::Value,
    after: serde_json::Value,
    pub scan_entries: u64,
}

pub struct Transcript {
    pub first: FirstPage,
    resumed: Frames,
    control: Frames,
    recovery: Option<(Frames, Frames)>,
    resumed_scan_entries: u64,
    released: serde_json::Value,
}

fn seed(cassie: &Cassie) -> (CassieSession, String) {
    let admin = cassie
        .authenticate_role("root", Some(crate::support_sql::test_root_password()), None)
        .expect("authenticated administrator");
    let password = uuid::Uuid::new_v4().to_string();
    for sql in [
        "CREATE TABLE wire_copy_denied (n INT)".to_owned(),
        format!("CREATE ROLE boundary_reader LOGIN PASSWORD '{password}'"),
        "GRANT CONNECT ON DATABASE postgres TO boundary_reader".to_owned(),
        "INSERT INTO wire_copy_denied (n) VALUES (1), (2), (3)".to_owned(),
    ] {
        cassie
            .execute_sql(&admin, &sql, Vec::new())
            .expect("fixture SQL");
    }
    (admin, password)
}

async fn first_page(cassie: &Cassie, client: &mut Client, scenario: Scenario) -> FirstPage {
    wire::transaction_control(&mut client.reader, &mut client.writer, "BEGIN").await;
    let before = cassie.metrics();
    let before_scan = cassie.midge.query_scan_entries_for_diagnostics();
    let frames = client
        .cycle(vec![
            wire::parse_frame("auth_s", scenario.query()),
            wire::bind_frame("auth_p", "auth_s", &[]),
            wire::execute_limited_frame("auth_p", 1),
            wire::sync_frame(),
        ])
        .await;
    let after = cassie.metrics();
    let scan_entries = cassie.midge.query_scan_entries_for_diagnostics() - before_scan;
    // This actual completed first page is the revocation barrier.
    assert_eq!(wire::error_code(&frames), None, "{frames:?}");
    assert_eq!(
        frames.iter().map(|frame| frame.0).collect::<Vec<_>>(),
        b"12TDsZ"
    );
    assert_eq!(
        frames.last().map(|frame| frame.1.as_slice()),
        Some(&b"T"[..])
    );
    let rows = wire::data_rows(&frames);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].len(), 1);
    assert!(matches!(rows[0][0].as_deref(), Some("1" | "2" | "3")));
    FirstPage {
        frames,
        before,
        after,
        scan_entries,
    }
}

async fn wait_for_retirement(cassie: &Cassie, finished: u64) -> serde_json::Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let metrics = cassie.metrics();
            if metrics["pgwire"]["active_sessions"].as_u64() == Some(0)
                && metrics["pgwire"]["sessions_finished_total"].as_u64() == Some(finished + 2)
                && metrics["pgwire"]["prepared_statements"].as_u64() == Some(0)
                && metrics["pgwire"]["portals"].as_u64() == Some(0)
                && metrics["query"]["current_accounted_memory_bytes"].as_u64() == Some(0)
                && metrics["runtime"]["running_queries"].as_u64() == Some(0)
                && metrics["runtime"]["active_operator_workers"].as_u64() == Some(0)
            {
                break metrics;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("both network owners and retained portal resources retire")
}

async fn observe(fixture: &SqlFixture, scenario: Scenario) -> Transcript {
    let (admin, password) = seed(&fixture.cassie);
    let server = wire::spawn_server(fixture.cassie.clone()).await;
    let mut owner = Client::connect(server.addr, "boundary_reader", &password).await;
    let mut control = Client::connect(
        server.addr,
        "root",
        crate::support_sql::test_root_password(),
    )
    .await;
    let first = first_page(&fixture.cassie, &mut owner, scenario).await;
    fixture
        .cassie
        .execute_sql(&admin, scenario.mutation(), Vec::new())
        .expect("change current access after completed first page");
    let before_resume = fixture.cassie.midge.query_scan_entries_for_diagnostics();
    // No Bind: resetting the portal would miss its retained execution state.
    let resumed = owner
        .cycle(vec![
            wire::execute_limited_frame("auth_p", 1),
            wire::sync_frame(),
        ])
        .await;
    let resumed_scan_entries =
        fixture.cassie.midge.query_scan_entries_for_diagnostics() - before_resume;
    let control_frames = control
        .cycle(vec![wire::simple_query_frame(
            "SELECT n FROM wire_copy_denied ORDER BY n",
        )])
        .await;
    let recovery = if matches!(scenario, Scenario::MaterializedDrop) {
        None
    } else {
        // Ordinary ROLLBACK also checks current access, so regrant it first.
        fixture
            .cassie
            .execute_sql(
                &admin,
                "GRANT CONNECT ON DATABASE postgres TO boundary_reader",
                Vec::new(),
            )
            .expect("restore CONNECT before transaction recovery");
        let rollback = owner
            .cycle(vec![wire::simple_query_frame("ROLLBACK")])
            .await;
        let restored = owner
            .cycle(vec![wire::simple_query_frame(
                "SELECT n FROM wire_copy_denied ORDER BY n",
            )])
            .await;
        Some((rollback, restored))
    };
    let finished = fixture.cassie.metrics()["pgwire"]["sessions_finished_total"]
        .as_u64()
        .expect("finished sessions before disconnect");
    drop((owner, control));
    let released = wait_for_retirement(&fixture.cassie, finished).await;
    server.stop().await;
    Transcript {
        first,
        resumed,
        control: control_frames,
        recovery,
        resumed_scan_entries,
        released,
    }
}

pub fn collect(scenario: Scenario) -> Transcript {
    let mut config = CassieRuntimeConfig::from_env().expect("configuration");
    crate::support_sql::test_root_password().clone_into(&mut config.password);
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    let fixture = sql_fixture_with_config("portal-current-authorization", &[], config);
    fixture.cassie.startup().expect("startup");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime");
    let transcript = runtime.block_on(observe(&fixture, scenario));
    // Runtime tasks and session/storage owners retire before strict directory cleanup.
    drop(runtime);
    drop(fixture);
    transcript
}

pub fn assert_denied(transcript: &Transcript) {
    // The final denial oracle runs after connection and fixture cleanup, also on RED.
    assert_eq!(
        wire::error_code(&transcript.resumed).as_deref(),
        Some("42501"),
        "resumed transcript: {:?}",
        transcript.resumed
    );
    assert_eq!(
        wire::data_rows(&transcript.resumed),
        [] as [Vec<Option<String>>; 0]
    );
    assert_eq!(
        transcript
            .resumed
            .iter()
            .map(|frame| frame.0)
            .collect::<Vec<_>>(),
        b"EZ"
    );
    assert_eq!(
        transcript.resumed.last().map(|frame| frame.1.as_slice()),
        Some(&b"E"[..])
    );
    assert_eq!(transcript.resumed_scan_entries, 0);
    let expected = vec![
        vec![Some("1".to_owned())],
        vec![Some("2".to_owned())],
        vec![Some("3".to_owned())],
    ];
    assert_eq!(wire::error_code(&transcript.control), None);
    assert_eq!(wire::data_rows(&transcript.control), expected);
    if let Some((rollback, restored)) = &transcript.recovery {
        assert_eq!(wire::error_code(rollback), None);
        assert_eq!(
            rollback.last().map(|frame| frame.1.as_slice()),
            Some(&b"I"[..])
        );
        assert_eq!(wire::error_code(restored), None);
        assert_eq!(wire::data_rows(restored), expected);
    }
    assert_eq!(
        transcript.released["pgwire"]["active_sessions"].as_u64(),
        Some(0)
    );
}

pub fn assert_materialized_first_page(first: &FirstPage) {
    assert_eq!(
        wire::data_rows(&first.frames),
        vec![vec![Some("1".to_owned())]]
    );
    let count = first.before["query"]["count"]
        .as_u64()
        .expect("before query count");
    let rows = first.before["query"]["rows_returned_total"]
        .as_u64()
        .expect("before source rows");
    assert_eq!(first.after["query"]["count"].as_u64(), Some(count + 1));
    assert_eq!(
        first.after["query"]["rows_returned_total"].as_u64(),
        Some(rows + 3)
    );
}
