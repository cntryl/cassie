//! The original persisted materialized-portal fixture with a measured source cap.

use crate::support_pgwire as wire;
use crate::support_sql_fixture::{sql_fixture_with_config, SqlFixture};
use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

pub const SQL: &str =
    "SELECT lower(payload) AS payload FROM portal_shared_memory WHERE payload IS NOT NULL";
pub type Frames = Vec<(u8, Vec<u8>)>;

pub struct QueryObservation {
    pub count: u64,
    pub rows_returned_total: u64,
    pub errors_total: u64,
    pub peak_accounted_memory_bytes: u64,
}

pub struct Fixture {
    pub storage: SqlFixture,
    config: CassieRuntimeConfig,
}

impl Fixture {
    pub fn new(label: &str, budget: Option<usize>) -> Self {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.max_result_rows = 1_000;
        if let Some(budget) = budget {
            config.limits.query_memory_budget_bytes = budget;
        }
        config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
        config.limits.parallel_scan_workers = 1;
        let storage = sql_fixture_with_config(label, &[], config.clone());
        storage.cassie.startup().expect("startup");
        storage
            .execute("CREATE TABLE portal_shared_memory (payload TEXT)")
            .expect("create table");
        let rows = (0..64)
            .map(|index| {
                (
                    Some(format!("doc-{index:04}")),
                    serde_json::json!({
                        "payload": format!("{index:04}-{}", "x".repeat(1_024)),
                    }),
                )
            })
            .collect();
        storage
            .cassie
            .midge
            .put_fresh_documents("portal_shared_memory", rows)
            .expect("seed original rows");
        Self { storage, config }
    }

    pub fn budget(&self) -> usize {
        self.config.limits.query_memory_budget_bytes
    }

    pub fn query(&self) -> QueryObservation {
        let snapshot = self.storage.cassie.metrics();
        let query = &snapshot["query"];
        QueryObservation {
            count: query["count"].as_u64().expect("query success count"),
            rows_returned_total: query["rows_returned_total"]
                .as_u64()
                .expect("source rows count"),
            errors_total: query["errors_total"].as_u64().expect("source errors count"),
            peak_accounted_memory_bytes: query["peak_accounted_memory_bytes"]
                .as_u64()
                .expect("source peak"),
        }
    }

    pub async fn connect(&self) -> Client {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind listener");
        let address = listener.local_addr().expect("listener address");
        drop(listener);
        let server = tokio::spawn(cassie::pgwire::server::run(
            address.to_string(),
            Arc::new(self.storage.cassie.clone()),
            self.config.clone(),
        ));
        // Preserve the existing server fixture's startup wait.
        tokio::time::sleep(Duration::from_millis(50)).await;
        let socket = tokio::net::TcpStream::connect(address)
            .await
            .expect("connect");
        let (reader, writer) = socket.into_split();
        let mut client = Client {
            reader,
            writer,
            server,
        };
        wire::complete_startup(&mut client.reader, &mut client.writer).await;
        client.control("BEGIN", b'T').await;
        let parsed = client
            .cycle(vec![
                wire::parse_frame("memory_stmt", SQL),
                wire::sync_frame(),
            ])
            .await;
        assert_eq!(wire::error_code(&parsed), None);
        assert_eq!(
            parsed.iter().map(|frame| frame.0).collect::<Vec<_>>(),
            b"1Z"
        );
        assert_eq!(
            parsed.last().map(|frame| frame.1.as_slice()),
            Some(&b"T"[..])
        );
        client
    }

    pub fn assert_committed_cleanup(&self) {
        let snapshot = self.storage.cassie.metrics();
        assert_eq!(
            snapshot["pgwire"]["portals"].as_u64(),
            Some(0),
            "COMMIT clears retained portals"
        );
        // Query controls observe source ownership, not private portal-memory bytes.
        assert_eq!(
            snapshot["query"]["current_accounted_memory_bytes"].as_u64(),
            Some(0)
        );
        assert_eq!(snapshot["runtime"]["running_queries"].as_u64(), Some(0));
        assert_eq!(
            snapshot["runtime"]["active_operator_workers"].as_u64(),
            Some(0)
        );
    }
}

pub struct Client {
    reader: OwnedReadHalf,
    writer: OwnedWriteHalf,
    server: tokio::task::JoinHandle<Result<(), cassie::app::CassieError>>,
}

impl Client {
    pub async fn cycle(&mut self, frames: Vec<Vec<u8>>) -> Frames {
        wire::write_frames(&mut self.writer, frames).await;
        wire::read_frames_until_ready_within(&mut self.reader, Duration::from_secs(5)).await
    }

    pub async fn control(&mut self, sql: &str, ready: u8) {
        let frames = self.cycle(vec![wire::simple_query_frame(sql)]).await;
        assert_eq!(wire::error_code(&frames), None, "{sql}: {frames:?}");
        assert_eq!(
            frames.iter().map(|frame| frame.0).collect::<Vec<_>>(),
            b"CZ"
        );
        assert_eq!(
            frames.last().map(|frame| frame.1.as_slice()),
            Some(&[ready][..])
        );
    }

    pub async fn page(&mut self, portal: &str) -> Frames {
        self.cycle(vec![
            wire::bind_frame(portal, "memory_stmt", &[]),
            wire::execute_limited_frame(portal, 1),
            wire::sync_frame(),
        ])
        .await
    }

    pub async fn stop(self) {
        drop((self.reader, self.writer));
        self.server.abort();
        let _ = self.server.await;
    }
}

pub fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}
