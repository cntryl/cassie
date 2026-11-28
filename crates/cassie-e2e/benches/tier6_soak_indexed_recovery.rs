use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use cassie::{app::Cassie, config::CassieRuntimeConfig, types::Value};
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tokio_postgres::{Client, NoTls};

#[path = "support/fixture_dir.rs"]
mod fixture_dir;
#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;

const INSERT: &str = "INSERT INTO soak_events (lane, seq, marker) VALUES ($1, $2, $3)";
const LOOKUP: &str = "SELECT lane, seq FROM soak_events WHERE marker = $1";
const FLUSH_INTERVAL: Duration = Duration::from_secs(5);
const OPERATION_TIMEOUT: Duration = Duration::from_secs(60);

struct Fixture {
    cassie: Arc<Cassie>,
    address: std::net::SocketAddr,
    shutdown: Arc<Notify>,
    server: tokio::task::JoinHandle<Result<(), cassie::app::CassieError>>,
}

impl Fixture {
    async fn open(path: &Path) -> Self {
        let config = CassieRuntimeConfig::from_env().expect("load Cassie benchmark configuration");
        let cassie = Arc::new(
            Cassie::new_local_with_data_dir_and_config(path, config.clone())
                .expect("open local indexed soak database"),
        );
        cassie.startup().expect("recover indexed soak database");
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind pgwire");
        let address = listener.local_addr().expect("pgwire address");
        let shutdown = Arc::new(Notify::new());
        let server = tokio::spawn(cassie::pgwire::server::run_with_listener_and_shutdown(
            listener,
            cassie.clone(),
            config,
            shutdown.clone(),
        ));
        Self {
            cassie,
            address,
            shutdown,
            server,
        }
    }

    async fn connect(&self) -> Client {
        let config = CassieRuntimeConfig::from_env().expect("load pgwire client configuration");
        let (client, connection) = tokio_postgres::Config::new()
            .host("127.0.0.1")
            .port(self.address.port())
            .user("root")
            .dbname(&config.database)
            .password(&config.password)
            .connect_timeout(OPERATION_TIMEOUT)
            .connect(NoTls)
            .await
            .expect("connect indexed soak client");
        tokio::spawn(async move { connection.await.expect("drive indexed soak connection") });
        client
    }

    async fn shutdown(self) {
        self.shutdown.notify_one();
        tokio::time::timeout(OPERATION_TIMEOUT, self.server)
            .await
            .expect("pgwire shutdown deadline")
            .expect("join pgwire server")
            .expect("stop pgwire server");
        tokio::time::timeout(OPERATION_TIMEOUT, async {
            while Arc::strong_count(&self.cassie) != 1 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("drain pgwire handlers before closing storage");
        assert_eq!(
            self.cassie.metrics()["pgwire"]["active_sessions"].as_u64(),
            Some(0)
        );
        self.cassie.shutdown();
        let storage = self.cassie.midge.clone();
        drop(self.cassie);
        tokio::task::spawn_blocking(move || storage.shutdown_owned_for_test(OPERATION_TIMEOUT))
            .await
            .expect("join storage shutdown")
            .expect("shut down storage and release its writer lease");
    }

    fn assert_index_lookup(&self) {
        let session = self.cassie.create_session("root", None);
        let plan = self
            .cassie
            .execute_sql(
                &session,
                &format!("EXPLAIN {LOOKUP}"),
                vec![Value::String("lane-1-event-0".to_owned())],
            )
            .expect("explain marker lookup");
        assert!(
            matches!(plan.rows().first().and_then(|row| row.first()), Some(Value::String(text)) if text.contains("soak_events_marker_idx")),
            "marker lookup must select the secondary index: {:?}",
            plan.rows(),
        );
    }

    fn assert_resource_bounds(&self, memory_limit: usize) {
        let metrics = self.cassie.metrics();
        assert_eq!(metrics["execution_result_cache"]["hits"].as_u64(), Some(0));
        assert_eq!(
            metrics["execution_result_cache"]["entries"].as_u64(),
            Some(0)
        );
        let peak = metrics["query"]["peak_accounted_memory_bytes"]
            .as_u64()
            .expect("query memory metric");
        assert!(peak <= u64::try_from(memory_limit).expect("query memory limit"));
        assert_eq!(metrics["runtime"]["running_queries"].as_u64(), Some(0));
        assert_eq!(
            metrics["runtime"]["active_operator_workers"].as_u64(),
            Some(0)
        );
    }

    async fn flush(&self) {
        let cassie = Arc::clone(&self.cassie);
        tokio::task::spawn_blocking(move || {
            cassie.midge.flush_data_family_for_collection("soak_events")
        })
        .await
        .expect("join data flush")
        .expect("flush retained rows");
    }
}

async fn insert(client: &Client, lane: i32, seq: i64) {
    let marker = format!("lane-{lane}-event-{seq}");
    assert_eq!(
        client
            .execute(INSERT, &[&lane, &seq, &marker])
            .await
            .expect("insert retained row"),
        1
    );
}

async fn lookup(client: &Client, lane: i32, seq: i64) {
    let marker = format!("lane-{lane}-event-{seq}");
    let row = client
        .query_one(LOOKUP, &[&marker])
        .await
        .expect("lookup retained row");
    assert_eq!((row.get::<_, i32>(0), row.get::<_, i64>(1)), (lane, seq));
}

struct Traffic {
    left: Client,
    right: Client,
    reader: Client,
    next_seq: i64,
    last_flush: Option<Instant>,
}

impl Traffic {
    async fn seed(fixture: &Fixture) -> Self {
        let (left, right, reader) =
            tokio::join!(fixture.connect(), fixture.connect(), fixture.connect());
        for statement in [
            "CREATE TABLE soak_events (lane INT NOT NULL, seq BIGINT NOT NULL, marker TEXT NOT NULL, PRIMARY KEY (lane, seq))",
            "CREATE INDEX soak_events_marker_idx ON soak_events (marker)",
        ] {
            left.batch_execute(statement).await.expect("seed indexed soak fixture");
        }
        for first in (1..=10_000).step_by(1_000) {
            let last = first + 999;
            left.batch_execute(&format!(
                "INSERT INTO soak_events (lane, seq, marker)
                 SELECT 0, CAST(n AS BIGINT), 'seed-' || CAST(n AS TEXT)
                 FROM generate_series({first}, {last}) AS seed(n)"
            ))
            .await
            .expect("append indexed fixture batch");
        }
        tokio::join!(insert(&left, 1, 0), insert(&right, 2, 0));
        fixture.assert_index_lookup();
        Self {
            left,
            right,
            reader,
            next_seq: 1,
            last_flush: None,
        }
    }

    async fn measure(&mut self, fixture: &Fixture, duration: Duration) -> stress::ExternalSample {
        let started = Instant::now();
        let initial_seq = self.next_seq;
        while started.elapsed() < duration || self.next_seq == initial_seq {
            let seq = self.next_seq;
            let traffic = async {
                tokio::join!(
                    insert(&self.left, 1, seq),
                    insert(&self.right, 2, seq),
                    lookup(&self.reader, 1, seq - 1)
                );
            };
            tokio::time::timeout(OPERATION_TIMEOUT, async {
                if self
                    .last_flush
                    .is_none_or(|last| last.elapsed() >= FLUSH_INTERVAL)
                {
                    tokio::join!(traffic, fixture.flush());
                    self.last_flush = Some(Instant::now());
                } else {
                    traffic.await;
                }
            })
            .await
            .expect("indexed soak made no progress before the operation deadline");
            self.next_seq += 1;
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        stress::ExternalSample::new(
            started.elapsed(),
            u64::try_from((self.next_seq - initial_seq) * 3).expect("bounded operation count"),
        )
    }
}

async fn verify_reopen(fixture: Fixture, traffic: Traffic, path: &Path) {
    let seq = traffic.next_seq;
    drop(traffic);
    fixture.shutdown().await;
    let reopened = Fixture::open(path).await;
    reopened.assert_index_lookup();
    let reader = reopened.connect().await;
    for lane in [1_i32, 2] {
        let row = reader
            .query_one(
                "SELECT COUNT(*), MIN(seq), MAX(seq) FROM soak_events WHERE lane = $1",
                &[&lane],
            )
            .await
            .expect("reconcile committed rows after reopening");
        assert_eq!(
            (
                row.get::<_, i64>(0),
                row.get::<_, i64>(1),
                row.get::<_, i64>(2)
            ),
            (seq, 0, seq - 1),
        );
        for position in [0, seq / 2, seq - 1] {
            lookup(&reader, lane, position).await;
        }
    }
    drop(reader);
    reopened.shutdown().await;
}

fn main() {
    std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
    std::env::set_var(
        "CASSIE_QUERY_MEMORY_BUDGET_BYTES",
        (64 * 1024 * 1024).to_string(),
    );
    std::env::set_var("CASSIE_ROOT_PASSWORD", uuid::Uuid::new_v4().to_string());
    let memory_limit = CassieRuntimeConfig::from_env()
        .expect("load benchmark limits")
        .limits
        .query_memory_budget_bytes;
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier6,
        "tier6_soak_indexed_recovery",
    );
    let case = stress::StressCase::new("indexed_ingest_reopen", "10k")
        .runtime_contract(
            stress::FixtureDeclaration::new(
                performance_benchmarks::FixtureClass::Soak,
                10_000,
                "tier6_soak_indexed_recovery/10k",
            ),
            stress::OperationUnit::Operation,
        )
        .metadata("trust_class", "diagnostic")
        .metadata("measurement_shape", "growing_indexed_dataset")
        .metadata("result_cardinality", "1")
        .metadata("query_memory_budget_bytes", memory_limit.to_string());
    if runner.is_enabled(&case) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("indexed soak runtime");
        let path =
            std::env::temp_dir().join(format!("cassie-indexed-soak-{}", uuid::Uuid::new_v4()));
        let _directory = fixture_dir::FixtureDir::new(&path);
        let fixture = runtime.block_on(Fixture::open(&path));
        let mut traffic = runtime.block_on(Traffic::seed(&fixture));
        let evidence = Arc::downgrade(&fixture.cassie);
        let case = case
            .preflight_evidence("index_seek", "none")
            .runtime_evidence(move || evidence.upgrade().expect("live soak fixture").metrics());
        runner.record_external(case, |duration| {
            let sample = runtime.block_on(traffic.measure(&fixture, duration));
            fixture.assert_resource_bounds(memory_limit);
            sample
        });
        runtime.block_on(verify_reopen(fixture, traffic, &path));
    }
    runner.finish();
}
