#[cfg(test)]
mod tests {
    use super::{backlog_at_window_end, percentile, profile_from, scheduled_arrivals};
    use std::time::Duration;

    #[test]
    fn should_choose_a_deterministic_product_smoke_profile() {
        // Arrange / Act
        let selected = profile_from(Some("smoke"));

        // Assert
        assert_eq!(selected.name, "smoke");
        assert_eq!(selected.duration, Duration::from_secs(1));
    }

    #[test]
    fn should_calculate_percentile_from_individual_request_samples() {
        // Arrange
        let samples = [
            Duration::from_micros(1),
            Duration::from_micros(2),
            Duration::from_micros(3),
            Duration::from_micros(4),
        ];

        // Act
        let p95 = percentile(&samples, 95);

        // Assert
        assert_eq!(p95, Duration::from_micros(4));
    }

    #[test]
    fn should_count_fixed_rate_arrivals_and_acknowledgements_separately() {
        // Arrange
        let window = Duration::from_millis(100);

        // Act
        let scheduled = scheduled_arrivals(20, window);

        // Assert
        assert_eq!(scheduled, 2);
        assert_eq!(scheduled_arrivals(2_000, Duration::from_secs(60)), 120_000);
        assert_eq!(backlog_at_window_end(3, 2), 1);
        assert_eq!(backlog_at_window_end(2, 3), 0);
    }
}

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use cassie::app::{Cassie, CassieError};
use cassie::config::CassieRuntimeConfig;
use serde::Serialize;
use tokio::sync::Notify;
use tokio_postgres::NoTls;

pub struct PgwireFixture {
    pub port: u16,
    pub cassie: Arc<Cassie>,
    shutdown: Arc<Notify>,
    server: tokio::task::JoinHandle<Result<(), CassieError>>,
}

impl PgwireFixture {
    pub async fn start(cassie: Arc<Cassie>) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind pgwire fixture listener");
        let address = listener.local_addr().expect("pgwire fixture address");
        let port = address.port();
        let shutdown = Arc::new(Notify::new());
        let config = CassieRuntimeConfig::from_env().expect("pgwire runtime configuration");
        let server = tokio::spawn(cassie::pgwire::server::run_with_listener_and_shutdown(
            listener,
            Arc::clone(&cassie),
            config,
            Arc::clone(&shutdown),
        ));
        let fixture = Self {
            port,
            cassie,
            shutdown,
            server,
        };
        fixture.wait_ready().await;
        fixture
    }

    async fn wait_ready(&self) {
        let config = CassieRuntimeConfig::from_env().expect("pgwire runtime configuration");
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let mut client = tokio_postgres::Config::new();
            client
                .host("127.0.0.1")
                .port(self.port)
                .user("root")
                .dbname(&config.database);
            if !config.password.is_empty() {
                client.password(&config.password);
            }
            if let Ok((_, connection)) = client.connect(NoTls).await {
                connection.abort();
                return;
            }
            assert!(
                Instant::now() < deadline,
                "pgwire fixture did not become ready"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    pub async fn prepared_clients(
        &self,
        sql: &str,
        count: usize,
    ) -> Vec<(
        tokio_postgres::Client,
        tokio::task::JoinHandle<()>,
        tokio_postgres::Statement,
    )> {
        let config = CassieRuntimeConfig::from_env().expect("pgwire runtime configuration");
        let mut clients = Vec::with_capacity(count);
        for _ in 0..count {
            let mut client_config = tokio_postgres::Config::new();
            client_config
                .host("127.0.0.1")
                .port(self.port)
                .user("root")
                .dbname(&config.database);
            if !config.password.is_empty() {
                client_config.password(&config.password);
            }
            let (client, connection) = client_config
                .connect(NoTls)
                .await
                .expect("connect prepared pgwire reader");
            let connection = tokio::spawn(async move {
                connection
                    .await
                    .expect("prepared pgwire connection remains healthy");
            });
            let statement = client.prepare(sql).await.expect("prepare pgwire reader");
            clients.push((client, connection, statement));
        }
        clients
    }

    pub async fn shutdown(self) {
        self.shutdown.notify_waiters();
        tokio::time::timeout(Duration::from_secs(2), self.server)
            .await
            .expect("pgwire server shuts down")
            .expect("pgwire server task")
            .expect("pgwire server result");
    }
}

#[derive(Clone, Copy)]
pub struct ProductProfile {
    pub name: &'static str,
    pub duration: Duration,
    pub rate_scale: u64,
    pub seeded_rows: usize,
}

pub fn profile() -> ProductProfile {
    profile_from(
        std::env::var("CASSIE_BENCH_PRODUCT_PROFILE")
            .ok()
            .as_deref(),
    )
}

fn profile_from(value: Option<&str>) -> ProductProfile {
    match value.unwrap_or("full") {
        "smoke" => ProductProfile {
            name: "smoke",
            duration: Duration::from_secs(1),
            rate_scale: 10,
            seeded_rows: 4_096,
        },
        "full" => ProductProfile {
            name: "full",
            duration: Duration::from_secs(60),
            rate_scale: 1,
            seeded_rows: 100_000,
        },
        other => panic!("unsupported CASSIE_BENCH_PRODUCT_PROFILE '{other}'"),
    }
}

pub fn output_dir() -> PathBuf {
    let path = std::env::var_os("CASSIE_BENCH_PRODUCT_OUTPUT_DIR")
        .map(PathBuf::from)
        .expect("set CASSIE_BENCH_PRODUCT_OUTPUT_DIR");
    std::fs::create_dir_all(&path).expect("create product benchmark output directory");
    path
}

pub fn write_json(directory: &Path, name: &str, value: &impl Serialize) -> PathBuf {
    let path = directory.join(name);
    let temporary = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(value).expect("serialize product benchmark evidence");
    std::fs::write(&temporary, bytes).expect("write product benchmark evidence");
    std::fs::rename(&temporary, &path).expect("publish product benchmark evidence");
    path
}

#[derive(Clone, Debug, Serialize)]
pub struct FixedRateReport {
    pub scheduled: u64,
    pub offered: u64,
    pub acknowledged: u64,
    pub acknowledged_at_window_end: u64,
    pub backlog_at_window_end: u64,
    pub generator_late: u64,
    pub acknowledged_after_window: u64,
    pub elapsed_millis: u128,
    pub drain_millis: u128,
    pub raw_latency_us: Vec<u64>,
}

pub fn run_fixed_rate(
    rate_per_second: u64,
    duration: Duration,
    mut write: impl FnMut(u64) -> Result<(), String>,
) -> FixedRateReport {
    assert!(rate_per_second > 0, "fixed rate must be positive");
    enum Message {
        Work(u64),
        Boundary {
            scheduled: u64,
            offered: u64,
            acknowledged: u64,
        },
    }
    let (sender, receiver) = mpsc::channel();
    let acknowledged = Arc::new(AtomicU64::new(0));
    let producer_acknowledged = Arc::clone(&acknowledged);
    let producer_start = Instant::now();
    let producer = thread::spawn(move || {
        let window_end = producer_start + duration;
        let scheduled = scheduled_arrivals(rate_per_second, duration);
        let mut offered = 0_u64;
        for sequence in 0..scheduled {
            let deadline = producer_start
                + Duration::from_nanos(sequence.saturating_mul(1_000_000_000) / rate_per_second);
            if let Some(wait) = deadline.checked_duration_since(Instant::now()) {
                thread::sleep(wait);
            }
            if Instant::now() >= window_end {
                break;
            }
            sender
                .send(Message::Work(sequence))
                .expect("workload writer remains active");
            offered = offered.saturating_add(1);
        }
        if let Some(wait) = window_end.checked_duration_since(Instant::now()) {
            thread::sleep(wait);
        }
        sender
            .send(Message::Boundary {
                scheduled,
                offered,
                acknowledged: producer_acknowledged.load(Ordering::Acquire),
            })
            .expect("workload writer records the measurement boundary");
        (scheduled, offered)
    });

    let mut offered_at_window_end = 0_u64;
    let mut acknowledged_at_window_end = 0_u64;
    let mut scheduled_at_window_end = 0_u64;
    let mut raw_latency_us = Vec::new();
    let window_end = producer_start + duration;
    let mut consumed = 0_u64;
    for message in receiver {
        match message {
            Message::Work(sequence) => {
                consumed = consumed.saturating_add(1);
                let started = Instant::now();
                write(sequence).unwrap_or_else(|error| panic!("fixed-rate write failed: {error}"));
                let elapsed = started.elapsed();
                raw_latency_us.push(u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX));
                acknowledged.fetch_add(1, Ordering::Release);
            }
            Message::Boundary {
                scheduled,
                offered,
                acknowledged,
            } => {
                scheduled_at_window_end = scheduled;
                offered_at_window_end = offered;
                acknowledged_at_window_end = acknowledged;
            }
        }
    }
    let (scheduled, offered) = producer.join().expect("fixed-rate producer completed");
    let acknowledged = acknowledged.load(Ordering::Acquire);
    assert_eq!(consumed, offered, "every offered workload item is consumed");
    let acknowledged_after_window = acknowledged.saturating_sub(acknowledged_at_window_end);
    let elapsed_millis = duration.as_millis();
    let drain_millis = Instant::now()
        .saturating_duration_since(window_end)
        .as_millis();
    FixedRateReport {
        scheduled,
        offered,
        acknowledged,
        acknowledged_at_window_end,
        backlog_at_window_end: backlog_at_window_end(
            scheduled_at_window_end,
            acknowledged_at_window_end,
        ),
        generator_late: scheduled.saturating_sub(offered_at_window_end),
        acknowledged_after_window,
        elapsed_millis,
        drain_millis,
        raw_latency_us,
    }
}

fn scheduled_arrivals(rate_per_second: u64, duration: Duration) -> u64 {
    let scheduled = duration
        .as_nanos()
        .saturating_mul(u128::from(rate_per_second))
        .div_ceil(1_000_000_000);
    u64::try_from(scheduled).unwrap_or(u64::MAX)
}

fn backlog_at_window_end(scheduled: u64, acknowledged: u64) -> u64 {
    scheduled.saturating_sub(acknowledged)
}

pub fn percentile(samples: &[Duration], percentile: usize) -> Duration {
    if samples.is_empty() {
        return Duration::ZERO;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    sorted[(sorted
        .len()
        .saturating_mul(percentile)
        .div_ceil(100)
        .saturating_sub(1))
    .min(sorted.len() - 1)]
}
