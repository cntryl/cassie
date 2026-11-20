//! Native-disk mixed projector, prepared range reader, and maintenance campaign.

#[path = "time_series_mixed/evidence.rs"]
mod evidence;
#[path = "time_series_mixed/fixture.rs"]
mod fixture;
#[path = "time_series_mixed/model.rs"]
mod model;
#[path = "time_series_mixed/workers.rs"]
mod workers;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use cntryl_stress::artifact::{BenchmarkMode, BenchmarkSpec, MeasurementIntent};
use cntryl_stress::{StressContext, StressRunner, StressRunnerConfig};

use self::evidence::HostProfile;
use self::fixture::Fixture;

const OWNER: &str = "tier5_time_series_mixed";
const SCALES: [(usize, &str); 2] = [(100_000, "100k"), (250_000, "250k")];
const CARDINALITIES: [usize; 2] = [16, 1_024];
const QUERY_BUDGET_BYTES: usize = fixture::QUERY_MEMORY_BUDGET_BYTES;

#[derive(Clone, Copy)]
enum RunKind {
    IngestOnly,
    ReaderOnly,
    Mixed,
}

impl RunKind {
    const ALL: [Self; 3] = [Self::IngestOnly, Self::ReaderOnly, Self::Mixed];

    const fn slug(self) -> &'static str {
        match self {
            Self::IngestOnly => "ingest_only",
            Self::ReaderOnly => "reader_only",
            Self::Mixed => "mixed",
        }
    }

    fn duration(self) -> Duration {
        match self {
            Self::IngestOnly => model::scaled(model::INGEST_SECONDS),
            Self::ReaderOnly => model::scaled(model::READER_SECONDS),
            Self::Mixed => model::scaled(model::MIXED_SECONDS),
        }
    }
}

pub fn run() {
    let host = HostProfile::capture_and_require();
    let mut runner = stress_runner(&host);
    let mut completed_scenarios = 0;
    let mut expected_scenarios = 0;
    let mut failures = Vec::new();
    let only = std::env::var("CASSIE_BENCH_TS_MIXED_ONLY").ok();

    for (event_count, scale) in SCALES {
        for device_count in CARDINALITIES {
            // Smoke runs may select one combination as `<scale>:<devices>`;
            // the qualification run leaves the filter unset.
            if only
                .as_deref()
                .is_some_and(|only| only != format!("{scale}:{device_count}"))
            {
                continue;
            }
            let mut sustainable_rate = None;
            let mut reader_baseline_p95 = None;
            // Harness smoke runs may pin the ingest-only point and run only the
            // mixed scenario; qualification runs never set this variable.
            let smoke_mixed_rate = std::env::var("CASSIE_BENCH_TS_MIXED_SMOKE_MIXED_ONLY_RATE")
                .ok()
                .and_then(|rate| rate.parse::<u64>().ok());
            if smoke_mixed_rate.is_some() {
                sustainable_rate = smoke_mixed_rate;
                reader_baseline_p95 = Some(1.0e12);
            }
            for kind in RunKind::ALL {
                if smoke_mixed_rate.is_some() && !matches!(kind, RunKind::Mixed) {
                    continue;
                }
                if std::env::var("CASSIE_BENCH_TS_MIXED_SMOKE_KIND")
                    .is_ok_and(|only_kind| only_kind != kind.slug())
                {
                    continue;
                }
                let report = run_scenario_spec(
                    &mut runner,
                    &host,
                    ScenarioInput {
                        scale,
                        event_count,
                        device_count,
                        kind,
                        sustainable_rate,
                        reader_baseline_p95,
                    },
                );
                expected_scenarios += 1;
                if let Some(report) = report {
                    completed_scenarios += 1;
                    failures.extend(report_failures(
                        &format!("{}.{scale}.{device_count}devices", kind.slug()),
                        kind,
                        sustainable_rate,
                        reader_baseline_p95,
                        &report,
                    ));
                    match kind {
                        RunKind::IngestOnly => sustainable_rate = Some(report.writer_rate),
                        RunKind::ReaderOnly => reader_baseline_p95 = Some(report.reader_p95_us),
                        RunKind::Mixed => {}
                    }
                }
            }
        }
    }

    let _run = runner.finish();
    assert_eq!(
        completed_scenarios, expected_scenarios,
        "the mixed qualification campaign requires every matched scenario"
    );
    assert!(
        failures.is_empty(),
        "qualification criteria failed after recording raw evidence: {failures:?}"
    );
}

fn stress_runner(host: &HostProfile) -> StressRunner {
    let mut config = StressRunnerConfig::from_env();
    config.samples = 1;
    config.warmup_samples = 0;
    config.cooldown_samples = 0;
    let mut metadata = BTreeMap::new();
    metadata.insert("deployment_profile_id".to_owned(), host.profile_id.clone());
    metadata.insert(
        "campaign_contract".to_owned(),
        "time_series_mixed_workload_v1".to_owned(),
    );
    metadata.insert("execution_result_cache".to_owned(), "disabled".to_owned());
    metadata.insert(
        "host_profile_json".to_owned(),
        serde_json::to_string(host).expect("serialize host profile metadata"),
    );
    StressRunner::with_config_and_metadata(OWNER, config, metadata)
}

#[derive(Clone, Copy)]
struct ScenarioInput<'a> {
    scale: &'a str,
    event_count: usize,
    device_count: usize,
    kind: RunKind,
    sustainable_rate: Option<u64>,
    reader_baseline_p95: Option<f64>,
}

fn run_scenario_spec(
    runner: &mut StressRunner,
    host: &HostProfile,
    input: ScenarioInput<'_>,
) -> Option<workers::ScenarioReport> {
    let spec = scenario_spec(
        input.scale,
        input.event_count,
        input.device_count,
        input.kind,
    );
    let host = host.clone();
    let output = Arc::new(Mutex::new(None));
    let output_capture = Arc::clone(&output);
    runner.run_spec(&spec, move |context| {
        let fixture = Fixture::seeded(
            input.scale,
            input.event_count,
            input.device_count,
            QUERY_BUDGET_BYTES,
        );
        let report = workers::run_scenario(
            &fixture,
            input.kind,
            input.kind.duration(),
            input.sustainable_rate,
            input.reader_baseline_p95,
        );
        record_scenario(context, &host, &fixture, input, &report);
        *output_capture.lock().expect("campaign result mutex") = Some(report);
    });
    let report = output.lock().expect("campaign result mutex").clone();
    report
}

fn record_scenario(
    context: &mut StressContext,
    host: &HostProfile,
    fixture: &Fixture,
    input: ScenarioInput<'_>,
    report: &workers::ScenarioReport,
) {
    let raw_path = evidence::write_report(host, fixture, input.kind, report);
    context
        .metadata("raw_evidence_path", raw_path.display())
        .metadata("offered_events", report.offered_events)
        .metadata("acknowledged_events", report.acknowledged_events)
        .metadata("peak_backlog_events", report.peak_backlog_events)
        .metadata("backlog_stable", report.backlog_stable)
        .metadata(
            "sustainable_ingest_events_per_second",
            report.sustainable_rate,
        )
        .metadata(
            "first_unsustainable_rate",
            report
                .first_unsustainable_rate
                .map_or_else(|| "none".to_owned(), |rate| rate.to_string()),
        )
        .metadata("target_writer_events_per_second", report.target_writer_rate)
        .metadata("reader_p50_us", report.reader_p50_us)
        .metadata("reader_p95_us", report.reader_p95_us)
        .metadata("rollup_debt", report.rollup_debt)
        .metadata(
            "first_limiting_resource",
            report.first_limiting_resource.clone(),
        )
        .metadata("rollup_refresh_completed", report.rollup_refresh_completed)
        .metadata("time_series_fallback_scans", report.fallback_scans)
        .metadata("sampled_peak_rss_bytes", report.sampled_peak_rss_bytes)
        .record_external(
            "campaign_window",
            report.elapsed,
            report.completed_operations.max(1),
        );
}

/// Collects every unmet qualification criterion so one failing scenario does
/// not discard the remaining matched runs; the campaign fails after all raw
/// evidence is written.
fn report_failures(
    label: &str,
    kind: RunKind,
    _sustainable_rate: Option<u64>,
    reader_baseline_p95: Option<f64>,
    report: &workers::ScenarioReport,
) -> Vec<String> {
    let mut failures = Vec::new();
    let mut require = |condition: bool, message: &str| {
        if !condition {
            failures.push(format!("{label}: {message}"));
        }
    };
    require(
        report.oracle_mismatches == 0,
        "independent relation/rollup oracle must have zero differences",
    );
    require(
        report.correction_events_acknowledged == report.correction_events_expected,
        "the out-of-order correction burst must drain",
    );
    require(
        report.acknowledged_events == report.offered_events,
        "the writer must drain all offered events",
    );
    require(
        report.backlog_stable,
        "writer backlog must drain or remain stable through the measured window",
    );
    require(
        !matches!(kind, RunKind::Mixed) || report.rollup_refresh_completed,
        "the minute-two rollup refresh must complete",
    );
    require(
        !report.pending_retention,
        "the scheduled retention cutoff must complete",
    );
    require(
        !matches!(kind, RunKind::IngestOnly) || report.sustainable_rate > 0,
        "the ingest-only bracket must find a sustainable rate",
    );
    if let Some(reader_baseline) = reader_baseline_p95 {
        require(
            report.reader_p95_us <= reader_baseline * 2.0,
            "mixed range-read p95 exceeds twice reader-only p95",
        );
    }
    failures
}

fn scenario_spec(scale: &str, events: usize, devices: usize, kind: RunKind) -> BenchmarkSpec {
    let id = format!(
        "perf.time_series.mixed_workload.{}.{}.{}devices",
        kind.slug(),
        scale,
        devices
    );
    let mut spec = BenchmarkSpec::new(
        id.clone(),
        id,
        5,
        BenchmarkMode::FixedDuration {
            sample_duration: kind.duration(),
        },
    );
    spec.intent = MeasurementIntent::External;
    spec.parameters
        .insert("seed_events".to_owned(), events.to_string());
    spec.parameters
        .insert("device_count".to_owned(), devices.to_string());
    let writer_count = usize::from(matches!(kind, RunKind::IngestOnly | RunKind::Mixed));
    let prepared_readers = if matches!(kind, RunKind::ReaderOnly | RunKind::Mixed) {
        4
    } else {
        0
    };
    let maintenance_workers = usize::from(matches!(kind, RunKind::Mixed));
    spec.parameters
        .insert("writer_count".to_owned(), writer_count.to_string());
    spec.parameters
        .insert("writer_batch_events".to_owned(), "128".to_owned());
    spec.parameters.insert(
        "prepared_reader_count".to_owned(),
        prepared_readers.to_string(),
    );
    spec.parameters.insert(
        "maintenance_worker_count".to_owned(),
        maintenance_workers.to_string(),
    );
    spec.parameters.insert(
        "query_memory_budget_bytes".to_owned(),
        QUERY_BUDGET_BYTES.to_string(),
    );
    spec.metadata.insert(
        "cache_policy".to_owned(),
        "execution_result_cache_disabled".to_owned(),
    );
    spec.metadata
        .insert("workload_mode".to_owned(), kind.slug().to_owned());
    spec.metadata.insert(
        "refresh_at_elapsed_seconds".to_owned(),
        model::scaled_seconds(model::REFRESH_AT_SECONDS).to_string(),
    );
    spec.metadata.insert(
        "correction_at_elapsed_seconds".to_owned(),
        model::scaled_seconds(model::CORRECTION_AT_SECONDS).to_string(),
    );
    spec.metadata.insert(
        "retention_at_elapsed_seconds".to_owned(),
        model::scaled_seconds(model::RETENTION_AT_SECONDS).to_string(),
    );
    spec.metadata.insert(
        "mixed_duration_seconds".to_owned(),
        model::scaled_seconds(model::MIXED_SECONDS).to_string(),
    );
    spec
}
