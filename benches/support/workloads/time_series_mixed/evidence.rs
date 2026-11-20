use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use super::fixture::{Fixture, QUERY_MEMORY_BUDGET_BYTES};
use super::workers::ScenarioReport;
use super::RunKind;

const PROFILE_ID: &str = "workstation-apple-m5-arm64-apfs";
const MINIMUM_FREE_BYTES: u64 = 20 * 1024 * 1024 * 1024;

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
        let profile_id = std::env::var("CASSIE_BENCH_DEPLOYMENT_PROFILE_ID")
            .expect("set CASSIE_BENCH_DEPLOYMENT_PROFILE_ID for native-disk qualification");
        assert_eq!(
            profile_id, PROFILE_ID,
            "this campaign requires the Apple M5/APFS profile"
        );
        assert!(cfg!(target_os = "macos"), "the campaign requires macOS");
        assert_eq!(
            std::env::consts::ARCH,
            "aarch64",
            "the campaign requires arm64"
        );

        let cpu = command_output("sysctl", &["-n", "machdep.cpu.brand_string"]);
        let memory = command_output("sysctl", &["-n", "hw.memsize"]);
        let memory_bytes = memory
            .trim()
            .parse::<u64>()
            .expect("sysctl hw.memsize should return bytes");
        assert!(
            cpu.contains("M5"),
            "the declared Apple M5 profile requires an M5 host"
        );
        assert!(
            memory_bytes >= 24 * 1024 * 1024 * 1024,
            "host memory is below the observed 24 GiB profile"
        );

        let filesystem_report = command_output("df", &["-Pk", "/private/tmp"]);
        let data_volume_free_bytes = available_bytes(&filesystem_report);
        assert!(
            data_volume_free_bytes >= MINIMUM_FREE_BYTES,
            "the campaign needs at least 20 GiB free on /private/tmp; found {data_volume_free_bytes} bytes"
        );
        let data_volume_device = filesystem_report
            .lines()
            .nth(1)
            .and_then(|line| line.split_whitespace().next())
            .expect("df -Pk should name the data volume device");
        let data_volume_report = command_output("diskutil", &["info", data_volume_device]);
        assert!(
            data_volume_report.to_ascii_uppercase().contains("APFS"),
            "the campaign requires /private/tmp on APFS"
        );

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
    let directory = Path::new("target/time-series-mixed-1350");
    std::fs::create_dir_all(directory).expect("create raw mixed-workload evidence directory");
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
    path
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
