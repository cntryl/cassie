use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use super::fixture::{Fixture, QUERY_MEMORY_BUDGET_BYTES};
use super::workers::ScenarioReport;
use super::RunKind;

const MINIMUM_FREE_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Clone, Debug, Serialize)]
pub struct HostProfile {
    pub profile_id: String,
    pub cpu: String,
    pub architecture: String,
    pub memory_bytes: u64,
    pub os: String,
    pub filesystem_report: String,
    pub data_volume_report: String,
    pub data_volume_free_bytes: u64,
}

impl HostProfile {
    pub fn capture_and_require() -> Self {
        let profile_id = std::env::var("CASSIE_BENCH_DEPLOYMENT_PROFILE_ID").unwrap_or_else(|_| {
            format!("native-{}-{}", std::env::consts::OS, std::env::consts::ARCH)
        });
        let cpu = if std::env::consts::OS == "macos" {
            command_output("sysctl", &["-n", "machdep.cpu.brand_string"])
        } else {
            command_output("uname", &["-m"])
        };
        let memory_bytes = if std::env::consts::OS == "macos" {
            command_output("sysctl", &["-n", "hw.memsize"])
                .trim()
                .parse::<u64>()
                .expect("parse macOS physical memory bytes")
        } else {
            linux_total_memory_bytes()
        };

        let filesystem_report = command_output("df", &["-Pk", "/tmp"]);
        let data_volume_free_bytes = available_bytes(&filesystem_report);
        assert!(
            data_volume_free_bytes >= MINIMUM_FREE_BYTES,
            "the campaign needs at least {MINIMUM_FREE_BYTES} bytes free on /tmp; found {data_volume_free_bytes} bytes"
        );
        let data_volume_report = filesystem_report.trim().to_owned();

        Self {
            profile_id,
            cpu: cpu.trim().to_owned(),
            architecture: std::env::consts::ARCH.to_owned(),
            memory_bytes,
            os: command_output("uname", &["-a"]).trim().to_owned(),
            filesystem_report: filesystem_report.trim().to_owned(),
            data_volume_report: data_volume_report.trim().to_owned(),
            data_volume_free_bytes,
        }
    }
}

fn linux_total_memory_bytes() -> u64 {
    let memory_kib = std::fs::read_to_string("/proc/meminfo")
        .expect("read Linux memory information")
        .lines()
        .find_map(|line| line.strip_prefix("MemTotal:"))
        .and_then(|value| value.split_whitespace().next())
        .and_then(|value| value.parse::<u64>().ok())
        .expect("parse Linux MemTotal in kibibytes");
    memory_kib
        .checked_mul(1024)
        .expect("Linux total memory bytes fit u64")
}

#[derive(Serialize)]
struct CampaignConfiguration<'a> {
    scenario: &'a str,
    seed_event_count: usize,
    device_count: usize,
    row_shape: &'static str,
    timestamp_shape: &'static str,
    time_series_index_bucket_width: &'static str,
    partition_key: &'static str,
    writers: usize,
    writer_batch_events: usize,
    prepared_readers: usize,
    reader_device_distribution: &'static str,
    maintenance_workers: usize,
    rollup_refresh_elapsed_seconds: u64,
    correction_elapsed_seconds: u64,
    correction_percent_of_seed: u64,
    retention_elapsed_seconds: u64,
    mixed_duration_seconds: u64,
    time_scale_percent: u64,
    query_memory_budget_bytes: usize,
    execution_result_cache_enabled: bool,
    storage_mode: &'static str,
    data_directory: &'a Path,
}

#[derive(Serialize)]
struct RawCampaign<'a> {
    captured_unix_nanos: u128,
    host: &'a HostProfile,
    configuration: CampaignConfiguration<'a>,
    report: &'a ScenarioReport,
}

pub fn write_report(
    host: &HostProfile,
    fixture: &Fixture,
    kind: RunKind,
    report: &ScenarioReport,
) -> PathBuf {
    let directory = std::env::var_os("CASSIE_BENCH_PRODUCT_OUTPUT_DIR").map_or_else(
        || PathBuf::from("target/time-series-mixed-1350"),
        PathBuf::from,
    );
    std::fs::create_dir_all(&directory).expect("create raw mixed-workload evidence directory");
    let id = uuid::Uuid::new_v4();
    let path = directory.join(format!(
        "{}-{}-{}devices-{id}.json",
        kind.slug(),
        fixture.event_count,
        fixture.device_count
    ));
    let captured_unix_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after Unix epoch")
        .as_nanos();
    let campaign = RawCampaign {
        captured_unix_nanos,
        host,
        configuration: CampaignConfiguration {
            scenario: kind.slug(),
            seed_event_count: fixture.event_count,
            device_count: fixture.device_count,
            row_shape: "id INT, device TEXT, event_at TIMESTAMP, amount INT",
            timestamp_shape:
                "14 deterministic UTC days; updates include backdated Jan 1 corrections",
            time_series_index_bucket_width: "1 day",
            partition_key: "device",
            writers: usize::from(matches!(kind, RunKind::IngestOnly | RunKind::Mixed)),
            writer_batch_events: super::model::WRITER_BATCH_EVENTS,
            prepared_readers: if matches!(kind, RunKind::ReaderOnly | RunKind::Mixed) {
                4
            } else {
                0
            },
            reader_device_distribution: "four readers partitioned round-robin across all devices",
            maintenance_workers: usize::from(matches!(kind, RunKind::Mixed)),
            rollup_refresh_elapsed_seconds: super::model::scaled_seconds(
                super::model::REFRESH_AT_SECONDS,
            ),
            correction_elapsed_seconds: super::model::scaled_seconds(
                super::model::CORRECTION_AT_SECONDS,
            ),
            correction_percent_of_seed: 2,
            retention_elapsed_seconds: super::model::scaled_seconds(
                super::model::RETENTION_AT_SECONDS,
            ),
            mixed_duration_seconds: super::model::scaled_seconds(super::model::MIXED_SECONDS),
            time_scale_percent: super::model::time_scale_percent(),
            query_memory_budget_bytes: QUERY_MEMORY_BUDGET_BYTES,
            execution_result_cache_enabled: false,
            storage_mode: "local",
            data_directory: &fixture.data_dir,
        },
        report,
    };
    let bytes =
        serde_json::to_vec_pretty(&campaign).expect("serialize raw mixed campaign evidence");
    std::fs::write(&path, bytes).expect("write raw mixed campaign evidence");
    if matches!(kind, RunKind::Mixed) {
        write_product_receipts(&path, report);
    }
    path
}

fn write_product_receipts(raw_path: &Path, report: &ScenarioReport) {
    let directory = raw_path.parent().expect("raw artifact parent");
    let profile = std::env::var("CASSIE_BENCH_PRODUCT_PROFILE").expect("product profile");
    let measurement_valid = report.oracle_mismatches == 0
        && report.acknowledged_events == report.offered_events
        && report.window_scheduled_events == report.window_offered_events
        && report.correction_events_expected == report.correction_events_acknowledged
        && report.reopened_page_checks > 0
        && report.maintenance.rollup_refresh_completed
        && !report.maintenance.pending_retention;
    let full = profile == "full";
    let raw_artifact = raw_path
        .file_name()
        .expect("raw artifact name")
        .to_string_lossy();
    for (name, target_met, measurements) in [
        (
            "time_series_dashboard.json",
            report.reader_p95_us <= 100_000.0,
            serde_json::json!({"reader_p50_us": report.reader_p50_us, "reader_p95_us": report.reader_p95_us, "raw_artifact": raw_artifact}),
        ),
        (
            "time_series_ingest.json",
            report.writer_rate >= 1_000 && report.backlog_growth_events <= 0,
            serde_json::json!({"scheduled_events": report.window_scheduled_events, "offered_events": report.window_offered_events, "acknowledged_events": report.window_acknowledged_events, "acknowledged_after_window": report.acknowledged_events.saturating_sub(report.window_acknowledged_events), "events_per_second": report.writer_rate, "backlog_growth_events": report.backlog_growth_events, "backlog_events_at_window_end": report.backlog_events_at_window_end, "raw_artifact": raw_artifact}),
        ),
    ] {
        let receipt = serde_json::json!({
            "metric": name.trim_end_matches(".json"),
            "profile": &profile,
            "measurement_valid": measurement_valid,
            "target_met": (full && measurement_valid).then_some(target_met),
            "measurements": measurements,
            "failures": if measurement_valid { Vec::<String>::new() } else { vec!["oracle, acknowledgement, rollup, or retention check failed".to_owned()] },
        });
        let path = directory.join(name);
        let temporary = path.with_extension("json.tmp");
        std::fs::write(
            &temporary,
            serde_json::to_vec_pretty(&receipt).expect("serialize product receipt"),
        )
        .expect("write product receipt");
        std::fs::rename(temporary, path).expect("publish product receipt");
    }
}

pub fn process_rss_bytes() -> u64 {
    let output = command_output("ps", &["-o", "rss=", "-p", &std::process::id().to_string()]);
    output
        .trim()
        .parse::<u64>()
        .unwrap_or_default()
        .saturating_mul(1024)
}

fn available_bytes(df_output: &str) -> u64 {
    let available_kib = df_output
        .lines()
        .nth(1)
        .and_then(|line| line.split_whitespace().nth(3))
        .and_then(|value| value.parse::<u64>().ok())
        .expect("df -Pk should expose available KiB");
    available_kib.saturating_mul(1024)
}

fn command_output(program: &str, arguments: &[&str]) -> String {
    let output = Command::new(program)
        .args(arguments)
        .output()
        .unwrap_or_else(|error| panic!("run {program} {arguments:?}: {error}"));
    assert!(
        output.status.success(),
        "{program} {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("host command output is UTF-8")
}
