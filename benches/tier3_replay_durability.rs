//! Local strict or buffered replay evidence. One process owns one fresh engine.
//! Results report foreground native commits and engine-wide sync counters separately.

use cassie::app::{Cassie, ProjectionReplayBatch, ProjectionReplayEvent};
use cassie::config::{CassieRuntimeConfig, CassieRuntimeLimits};
use cassie::midge::adapter::observe_native_commits;
use serde_json::json;
use std::time::Instant;

#[path = "tier3_replay_durability/mode.rs"]
mod mode;
use mode::{ReplayMode, Shape, Workload};
#[path = "tier3_replay_durability/verification.rs"]
mod verification;

#[path = "tier3_replay_durability/read_campaign.rs"]
mod read_campaign;

fn parameter(name: &str, default: usize) -> usize {
    let value = std::env::var(name).map_or(default, |value| {
        value
            .parse()
            .unwrap_or_else(|_| panic!("{name} must be a positive integer"))
    });
    assert!(value > 0, "{name} must be positive");
    value
}

fn prepare_batches(
    projection: &str,
    batches: usize,
    batch_size: usize,
) -> Vec<ProjectionReplayBatch> {
    (0..batches)
        .map(|batch| {
            let events = (0..batch_size)
                .map(|index| {
                    let number = batch
                        .checked_mul(batch_size)
                        .and_then(|n| n.checked_add(index + 1))
                        .expect("batch event count overflow");
                    ProjectionReplayEvent {
                        event_id: format!("event-{number}"),
                        checkpoint: format!("checkpoint-{number}"),
                        position: Some(u64::try_from(number).expect("event position fits u64")),
                        document_id: format!("doc-{number}"),
                        payload: Some(json!({"title": format!("title-{}", number % 16)})),
                    }
                })
                .collect();
            ProjectionReplayBatch {
                projection: projection.to_owned(),
                source_identity: "replay-benchmark".to_owned(),
                batch_id: format!("batch-{batch}"),
                lag: 0,
                events,
            }
        })
        .collect()
}

/// One projector transaction: the batch's rows and its checkpoint together.
fn sql_transaction(
    cassie: &Cassie,
    session: &cassie::app::CassieSession,
    mode: ReplayMode,
    batch: &ProjectionReplayBatch,
) -> (
    Result<(), cassie::app::CassieError>,
    cassie::midge::adapter::NativeCommitObservation,
) {
    let run = |sql: &str| cassie.execute_sql(session, sql, vec![]).map(|_| ());
    let values = batch
        .events
        .iter()
        .map(|event| {
            let title = event
                .payload
                .as_ref()
                .and_then(|payload| payload["title"].as_str());
            format!("('{}')", title.expect("benchmark payload title"))
        })
        .collect::<Vec<_>>()
        .join(", ");
    let position = batch
        .events
        .last()
        .and_then(|event| event.position)
        .expect("benchmark batch position");
    let staged = (|| {
        run("BEGIN")?;
        if mode == ReplayMode::SqlReplayable {
            run("SET LOCAL cassie.projection_write_mode = 'replayable'")?;
        }
        run(&format!("INSERT INTO replay_bench (title) VALUES {values}"))?;
        run(&format!(
            "UPDATE replay_bench_checkpoint SET position = {position}"
        ))
    })();
    if let Err(error) = staged {
        return (
            Err(error),
            cassie::midge::adapter::NativeCommitObservation::default(),
        );
    }
    observe_native_commits(|| run("COMMIT"))
}

/// Creates the measured table, its optional index and the SQL checkpoint row.
fn create_fixture(
    cassie: &Cassie,
    session: &cassie::app::CassieSession,
    mode: ReplayMode,
    shape: Shape,
) {
    let mut fixture = vec!["CREATE TABLE replay_bench (title TEXT)"];
    if shape == Shape::Indexed {
        fixture.push("CREATE INDEX replay_bench_title_idx ON replay_bench USING btree (title)");
    }
    if mode.is_sql() {
        fixture.push("CREATE TABLE replay_bench_checkpoint (position BIGINT)");
        fixture.push("INSERT INTO replay_bench_checkpoint (position) VALUES (0)");
    }
    for sql in fixture {
        cassie
            .execute_sql(session, sql, vec![])
            .expect("fixture DDL");
    }
}

/// Populates duplicate/no-op inputs before the measured counter window.
fn seed_workload(
    cassie: &Cassie,
    mode: ReplayMode,
    workload: Workload,
    inputs: &mut [ProjectionReplayBatch],
) {
    if workload != Workload::Apply {
        // Populate the same row/checkpoint shape before measurement. Duplicate
        // lanes repeat those event ids; no-op lanes submit empty batches.
        for batch in &*inputs {
            match mode {
                ReplayMode::Strict => cassie.replay_projection_batch_strict(batch.clone()),
                _ => cassie.replay_projection_batch(batch.clone()),
            }
            .expect("seed replay workload");
        }
        if workload == Workload::Noop {
            for batch in inputs {
                batch.events.clear();
            }
        }
    }
}

/// Measures one full acknowledgement; assertions retain the original boundaries.
fn measure_batch(
    cassie: &Cassie,
    session: &cassie::app::CassieSession,
    mode: ReplayMode,
    workload: Workload,
    batch: ProjectionReplayBatch,
    batch_size: usize,
) -> (u128, cassie::midge::adapter::NativeCommitObservation) {
    let start = Instant::now();
    let commits = if mode.is_sql() {
        let (result, commits) = sql_transaction(cassie, session, mode, &batch);
        result.expect("SQL projector transaction");
        commits
    } else {
        let (report, commits) = observe_native_commits(|| match mode {
            ReplayMode::Strict => cassie.replay_projection_batch_strict(batch),
            _ => cassie.replay_projection_batch(batch),
        });
        let report = report.expect("replay batch");
        assert_eq!(
            (report.applied_event_count, report.skipped_duplicate_count),
            workload.expected_counts(u64::try_from(batch_size).unwrap())
        );
        commits
    };
    let elapsed_ns = start.elapsed().as_nanos();
    assert_eq!(
        commits.first_policy,
        Some(mode.native_policy()),
        "native commit must match the selected benchmark profile"
    );
    (elapsed_ns, commits)
}

/// Replay-specific finite verification profile; production defaults stay unchanged.
fn benchmark_config() -> CassieRuntimeConfig {
    CassieRuntimeConfig {
        limits: CassieRuntimeLimits {
            query_memory_budget_bytes: 64 * 1024 * 1024,
            query_timeout_ms: 120_000,
            ..CassieRuntimeLimits::default()
        },
        ..CassieRuntimeConfig::default()
    }
}

fn handle_read_mode() -> bool {
    if let Some(source) = std::env::var_os("CASSIE_REPLAY_BENCH_SOURCE_FINGERPRINT") {
        read_campaign::print_source_fingerprint(std::path::Path::new(&source))
            .unwrap_or_else(|error| panic!("{error}"));
        return true;
    }
    if let Some(manifest) = std::env::var_os("CASSIE_REPLAY_BENCH_READ_MANIFEST") {
        read_campaign::run(std::path::Path::new(&manifest))
            .unwrap_or_else(|error| panic!("{error}"));
        return true;
    }
    false
}

fn main() {
    if handle_read_mode() {
        return;
    }
    let mode = ReplayMode::parse(std::env::var_os("CASSIE_REPLAY_BENCH_MODE").as_deref())
        .unwrap_or_else(|error| panic!("{error}"));
    let shape = Shape::parse(std::env::var_os("CASSIE_REPLAY_BENCH_SHAPE").as_deref())
        .unwrap_or_else(|error| panic!("{error}"));
    let workload = Workload::parse(std::env::var_os("CASSIE_REPLAY_BENCH_WORKLOAD").as_deref())
        .unwrap_or_else(|error| panic!("{error}"));
    workload
        .validate(mode)
        .unwrap_or_else(|error| panic!("{error}"));
    let batch_size = parameter("CASSIE_REPLAY_BENCH_BATCH_SIZE", 64);
    let batches = parameter("CASSIE_REPLAY_BENCH_BATCHES", 32);
    let total_events = batches
        .checked_mul(batch_size)
        .expect("event count overflow");
    let event_count = u32::try_from(total_events).expect("event count exceeds benchmark limit");
    let source = std::env::var("CASSIE_REPLAY_BENCH_SOURCE")
        .expect("set CASSIE_REPLAY_BENCH_SOURCE to the exact source and instrumentation identity");
    let path = std::env::temp_dir().join(format!("cassie-replay-bench-{}", uuid::Uuid::new_v4()));
    let config = benchmark_config();
    let query_memory_budget_bytes = config.limits.query_memory_budget_bytes;
    let query_timeout_ms = config.limits.query_timeout_ms;
    let setup_start = Instant::now();
    let cassie =
        Cassie::new_local_with_data_dir_and_config(&path, config.clone()).expect("local Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("benchmark", None);
    create_fixture(&cassie, &session, mode, shape);
    let projection = cassie.midge.canonical_collection_name("replay_bench");
    let mut inputs = prepare_batches(&projection, batches, batch_size);
    seed_workload(&cassie, mode, workload, &mut inputs);
    let setup_ns = setup_start.elapsed().as_nanos();
    let before = cassie
        .midge
        .native_durability_metrics()
        .expect("native metrics before");
    let mut samples = Vec::with_capacity(batches);
    let measured_start = Instant::now();
    for batch in inputs {
        samples.push(measure_batch(
            &cassie, &session, mode, workload, batch, batch_size,
        ));
    }
    let measured = measured_start.elapsed();
    let measured_ns = measured.as_nanos();
    let submitted_events = if workload == Workload::Noop {
        0
    } else {
        event_count
    };
    let events_per_second = (measured.as_secs_f64() > 0.0)
        .then(|| f64::from(submitted_events) / measured.as_secs_f64());
    let after = cassie
        .midge
        .native_durability_metrics()
        .expect("native metrics after");
    let verification_start = Instant::now();
    verification::verify(&cassie, mode, total_events);
    let verification_ns = verification_start.elapsed().as_nanos();
    drop(session);
    cassie.shutdown();
    drop(cassie);
    let recovery_ns = verification::clean_reopen(&path, config, mode, total_events);
    let samples: Vec<_> = samples.into_iter().map(|(elapsed_ns, commits)| {
        json!({"elapsed_ns": elapsed_ns.to_string(), "foreground_native_commits": commits})
    }).collect();
    println!(
        "{}",
        json!({
            "source": source,
            "mode": mode.label(),
            "shape": shape.label(),
            "workload": workload.label(),
            "selected_primary_policy": mode.native_policy(),
            "query_memory_budget_bytes": query_memory_budget_bytes,
            "query_timeout_ms": query_timeout_ms,
            "submitted_events": submitted_events,
            "applied_events": if workload == Workload::Apply { event_count } else { 0 },
            "verification_ns": verification_ns.to_string(),
            "recovery_ns": recovery_ns.to_string(),
            "recovery_boundary": "clean shutdown then constructor and startup; verification excluded",
            "replay_api": mode.api(),
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "storage_path": path,
            "batch_size": batch_size,
            "batch_count": batches,
            "events": total_events,
            "events_per_second": events_per_second,
            "setup_ns": setup_ns.to_string(),
            "measured_ns": measured_ns.to_string(),
            "native_before": before,
            "native_after": after,
            "samples": samples,
            "counter_scope": "foreground native commits; engine-wide syncs include background work and exclude unaccounted filesystem syncs"
        })
    );
}
