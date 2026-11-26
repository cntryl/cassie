#[path = "support/observation.rs"]
mod observation;

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use cassie_bench_support::{performance_benchmarks, stress};
use cassie_engine::{
    app::{Cassie, ProjectionReplayBatch, ProjectionReplayEvent},
    config::CassieRuntimeConfig,
    types::{DataType, FieldSchema, Schema},
};
use observation::ObservationBridge;
use serde_json::json;

const ROWS: usize = 2_048;
const COLLECTION: &str = "tier2_projection_replay";
const WRITE_COLLECTION: &str = "tier2_projection_write";
struct ReplayState {
    batches: VecDeque<ProjectionReplayBatch>,
    next_sample: u64,
}

fn main() {
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier2,
        "tier2_subsystem_ingest",
    );
    let replay_case = stress::StressCase::new("projection_replay_batch", "2k");
    let write_case = stress::StressCase::new("projection_write_batch", "2k");
    let replay_enabled = runner.is_enabled(&replay_case);
    let write_enabled = runner.is_enabled(&write_case);
    if !replay_enabled && !write_enabled {
        runner.finish();
        return;
    }

    let setup_started = Instant::now();
    let config = CassieRuntimeConfig::from_env().expect("projection replay config");
    let cassie = Arc::new(
        Cassie::new_in_memory_with_config(config).expect("projection replay memory fixture"),
    );
    cassie.startup().expect("projection replay startup");
    let schema = Schema {
        fields: vec![
            FieldSchema {
                name: "id".to_owned(),
                data_type: DataType::Text,
                nullable: false,
            },
            FieldSchema {
                name: "title".to_owned(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "score".to_owned(),
                data_type: DataType::Int,
                nullable: true,
            },
        ],
    };
    cassie
        .midge
        .create_collection(COLLECTION, schema.clone())
        .expect("create projection replay collection");
    cassie.register_collection(COLLECTION, schema.clone());
    cassie
        .midge
        .create_collection(WRITE_COLLECTION, schema.clone())
        .expect("create projection write collection");
    cassie.register_collection(WRITE_COLLECTION, schema);
    let write_documents = (0..ROWS)
        .map(|index| {
            let id = format!("tier2-write-{index}");
            (
                Some(id.clone()),
                json!({
                    "id": id,
                    "title": format!("title-{}", index % 16),
                    "score": index % 100,
                }),
            )
        })
        .collect::<Vec<_>>();

    let fixture = Arc::new(Mutex::new(ReplayState {
        batches: VecDeque::from([replay_batch(0)]),
        next_sample: 1,
    }));
    let setup_time_ns = setup_started.elapsed().as_nanos().to_string();
    let replay_case = replay_case
        .runtime_contract(
            stress::FixtureDeclaration::new(
                performance_benchmarks::FixtureClass::Subsystem,
                ROWS,
                "tier2_subsystem_ingest/2k",
            ),
            stress::OperationUnit::Event,
        )
        .metadata("setup_time_ns", &setup_time_ns);

    if replay_enabled {
        let evidence_cassie = Arc::clone(&cassie);
        runner.measure_counted(
            replay_case.runtime_evidence(move || evidence_cassie.metrics()),
            || {
                let batch = fixture
                    .lock()
                    .expect("projection replay fixture")
                    .batches
                    .pop_front()
                    .expect("prepared replay batch available");
                let applied = cassie
                    .replay_projection_batch(batch)
                    .expect("projection replay batch should apply");
                assert_eq!(
                    applied.applied_event_count,
                    u64::try_from(ROWS).expect("rows fit u64")
                );
                assert_eq!(applied.skipped_duplicate_count, 0);
                let next_fixture = Arc::clone(&fixture);
                ObservationBridge(
                    cassie_engine::benchmark::KernelObservation::new(
                        u64::try_from(ROWS).expect("rows fit u64"),
                        applied.applied_event_count,
                    )
                    .with_after_sample(move || {
                        let mut state = next_fixture.lock().expect("projection replay fixture");
                        let sample = state.next_sample;
                        state.next_sample = state.next_sample.saturating_add(1);
                        state.batches.push_back(replay_batch(sample));
                    }),
                )
            },
        );
    }
    if write_enabled {
        let evidence_cassie = Arc::clone(&cassie);
        let write_case = write_case
            .runtime_contract(
                stress::FixtureDeclaration::new(
                    performance_benchmarks::FixtureClass::Subsystem,
                    ROWS,
                    "tier2_subsystem_ingest/2k",
                ),
                stress::OperationUnit::Document,
            )
            .metadata("setup_time_ns", &setup_time_ns)
            .runtime_evidence(move || evidence_cassie.metrics());
        runner.measure_counted(write_case, || {
            let written = cassie
                .midge
                .put_documents(WRITE_COLLECTION, write_documents.clone())
                .expect("projection write batch should commit");
            assert_eq!(written.len(), ROWS);
            std::hint::black_box(written);
            ObservationBridge(cassie_engine::benchmark::KernelObservation::new(
                u64::try_from(ROWS).expect("rows fit u64"),
                u64::try_from(ROWS).expect("rows fit u64"),
            ))
        });
    }
    cassie.shutdown();
    runner.finish();
}

fn replay_batch(sample: u64) -> ProjectionReplayBatch {
    let events = (0..ROWS)
        .map(|index| {
            let index = u64::try_from(index).expect("event index fits u64");
            let ordinal = sample
                .saturating_mul(u64::try_from(ROWS).expect("rows fit u64"))
                .saturating_add(index)
                .saturating_add(1);
            let document_id = format!("tier2-replay-{sample}-{index}");
            ProjectionReplayEvent {
                event_id: format!("tier2-event-{sample}-{index}"),
                checkpoint: format!("tier2-checkpoint-{sample}-{index}"),
                position: Some(ordinal),
                document_id: document_id.clone(),
                payload: Some(json!({
                    "id": document_id,
                    "title": format!("title-{}", index % 16),
                    "score": index % 100,
                })),
            }
        })
        .collect();
    ProjectionReplayBatch {
        projection: COLLECTION.to_owned(),
        source_identity: "tier2-replay-source".to_owned(),
        batch_id: format!("tier2-replay-batch-{sample}"),
        lag: 0,
        events,
    }
}
