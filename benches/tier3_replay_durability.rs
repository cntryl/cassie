//! Local strict or buffered replay evidence. One process owns one fresh engine.
//! Results report foreground native commits and engine-wide sync counters separately.

use cassie::app::{Cassie, ProjectionReplayBatch, ProjectionReplayEvent};
use cassie::config::CassieRuntimeConfig;
use cassie::midge::adapter::observe_native_commits;
use serde_json::json;
use std::time::Instant;

#[path = "tier3_replay_durability/mode.rs"]
mod mode;
use mode::{ReplayMode, Shape};

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
            let title = event.payload.as_ref().and_then(|payload| payload["title"].as_str());
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
        run(&format!("UPDATE replay_bench_checkpoint SET position = {position}"))
    })();
    if let Err(error) = staged {
        return (Err(error), cassie::midge::adapter::NativeCommitObservation::default());
    }
    observe_native_commits(|| run("COMMIT"))
}

fn main() {
    let mode = ReplayMode::parse(std::env::var_os("CASSIE_REPLAY_BENCH_MODE").as_deref())
        .unwrap_or_else(|error| panic!("{error}"));
    let shape = Shape::parse(std::env::var_os("CASSIE_REPLAY_BENCH_SHAPE").as_deref())
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
    let setup_start = Instant::now();
    let cassie = Cassie::new_local_with_data_dir_and_config(&path, CassieRuntimeConfig::default())
        .expect("local Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("benchmark", None);
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
            .execute_sql(&session, sql, vec![])
            .expect("fixture DDL");
    }
    let projection = cassie.midge.canonical_collection_name("replay_bench");
    let inputs = prepare_batches(&projection, batches, batch_size);
    let setup_ns = setup_start.elapsed().as_nanos();
    let before = cassie
        .midge
        .native_durability_metrics()
        .expect("native metrics before");
    let mut samples = Vec::with_capacity(batches);
    let measured_start = Instant::now();
    for batch in inputs {
        let start = Instant::now();
        let commits = if mode.is_sql() {
            let (result, commits) = sql_transaction(&cassie, &session, mode, &batch);
            result.expect("SQL projector transaction");
            commits
        } else {
            let (report, commits) = observe_native_commits(|| match mode {
                ReplayMode::Strict => cassie.replay_projection_batch_strict(batch),
                _ => cassie.replay_projection_batch(batch),
            });
            let report = report.expect("replay batch");
            assert_eq!(
                report.applied_event_count,
                u64::try_from(batch_size).unwrap()
            );
            assert_eq!(report.skipped_duplicate_count, 0);
            commits
        };
        let elapsed_ns = start.elapsed().as_nanos();
        assert_eq!(
            commits.first_policy,
            Some(mode.native_policy()),
            "native commit must match the selected benchmark profile"
        );
        samples.push((elapsed_ns, commits));
    }
    let measured = measured_start.elapsed();
    let measured_ns = measured.as_nanos();
    let events_per_second =
        (measured.as_secs_f64() > 0.0).then(|| f64::from(event_count) / measured.as_secs_f64());
    let after = cassie
        .midge
        .native_durability_metrics()
        .expect("native metrics after");
    let samples: Vec<_> = samples.into_iter().map(|(elapsed_ns, commits)| {
        json!({"elapsed_ns": elapsed_ns.to_string(), "foreground_native_commits": commits})
    }).collect();
    println!(
        "{}",
        json!({
            "source": source,
            "mode": mode.label(),
            "shape": shape.label(),
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
    cassie.shutdown();
}
