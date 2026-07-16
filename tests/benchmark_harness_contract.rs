#[path = "../benches/support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "../benches/support/stress.rs"]
#[allow(dead_code)]
mod stress;

use std::collections::BTreeSet;
use std::time::Duration;
use std::{cell::Cell, panic::AssertUnwindSafe};

use serde_json::json;

fn tier1_row_case(
    fixture_class: performance_benchmarks::FixtureClass,
    fixture_rows: usize,
    operation_unit: stress::OperationUnit,
) -> stress::StressCase {
    stress::StressCase::new("row_encode_decode", "micro").runtime_contract(
        stress::FixtureDeclaration::new(
            fixture_class,
            fixture_rows,
            "tier1_hotpath_row_codec/micro",
        ),
        operation_unit,
    )
}

#[test]
fn should_allow_incomplete_selector_case_before_setup() {
    // Arrange
    let runner = stress::CassieStressRunner::new(
        "tier1_hotpath_row_codec",
        performance_benchmarks::BenchmarkTier::Tier1,
    );
    let selector = stress::StressCase::new("row_encode_decode", "micro");

    // Act
    let enabled = runner.is_enabled(&selector);

    // Assert
    assert!(enabled);
}

#[test]
fn should_type_every_registered_operation_unit() {
    // Arrange
    let registered = performance_benchmarks::benchmark_scenarios()
        .map(|scenario| scenario.operation_unit)
        .collect::<BTreeSet<_>>();

    // Act
    let typed = stress::OperationUnit::ALL
        .into_iter()
        .map(stress::OperationUnit::as_str)
        .collect::<BTreeSet<_>>();

    // Assert
    assert_eq!(typed, registered);
}

#[test]
fn should_construct_each_owner_with_explicit_benchmark_tier() {
    // Arrange
    let owners = performance_benchmarks::benchmark_scenarios()
        .map(|scenario| (scenario.benchmark, scenario.declared_tier))
        .collect::<BTreeSet<_>>();
    let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));

    // Act
    let implicit_owners = owners
        .into_iter()
        .filter_map(|(owner, tier)| {
            let source =
                std::fs::read_to_string(repository.join("benches").join(format!("{owner}.rs")))
                    .expect("benchmark owner source");
            let source = source.split_whitespace().collect::<String>();
            let constructor = format!(
                "stress::runner(performance_benchmarks::BenchmarkTier::Tier{},",
                tier.number()
            );
            (!source.contains(&constructor)).then_some(owner)
        })
        .collect::<Vec<_>>();
    let harness = include_str!("../benches/support/stress.rs");

    // Assert
    assert!(
        implicit_owners.is_empty(),
        "implicit owners: {implicit_owners:?}"
    );
    assert!(!harness.contains("benchmark_tier_from_owner"));
}

#[test]
fn should_reject_missing_runtime_declaration_before_measurement_closure() {
    // Arrange
    let invoked = Cell::new(false);
    let mut runner = stress::CassieStressRunner::new(
        "tier1_hotpath_row_codec",
        performance_benchmarks::BenchmarkTier::Tier1,
    );
    let case = stress::StressCase::new("row_encode_decode", "micro");

    // Act
    let panic = std::panic::catch_unwind(AssertUnwindSafe(|| {
        runner.measure_micro(case, || {
            invoked.set(true);
            1_usize
        });
    }));

    // Assert
    assert!(panic.is_err());
    assert!(!invoked.get());
}

#[test]
fn should_reject_runtime_fixture_class_mismatch_before_measurement_closure() {
    // Arrange
    let invoked = Cell::new(false);
    let mut runner = stress::CassieStressRunner::new(
        "tier1_hotpath_row_codec",
        performance_benchmarks::BenchmarkTier::Tier1,
    );
    let case = tier1_row_case(
        performance_benchmarks::FixtureClass::Subsystem,
        0,
        stress::OperationUnit::Row,
    );

    // Act
    let panic = std::panic::catch_unwind(AssertUnwindSafe(|| {
        runner.measure_micro(case, || {
            invoked.set(true);
            1_usize
        });
    }));

    // Assert
    assert!(panic.is_err());
    assert!(!invoked.get());
}

#[test]
fn should_reject_runtime_fixture_size_mismatch_before_measurement_closure() {
    // Arrange
    let invoked = Cell::new(false);
    let mut runner = stress::CassieStressRunner::new(
        "tier1_hotpath_row_codec",
        performance_benchmarks::BenchmarkTier::Tier1,
    );
    let case = tier1_row_case(
        performance_benchmarks::FixtureClass::Kernel,
        1,
        stress::OperationUnit::Row,
    );

    // Act
    let panic = std::panic::catch_unwind(AssertUnwindSafe(|| {
        runner.measure_micro(case, || {
            invoked.set(true);
            1_usize
        });
    }));

    // Assert
    assert!(panic.is_err());
    assert!(!invoked.get());
}

#[test]
fn should_reject_runtime_operation_unit_mismatch_before_measurement_closure() {
    // Arrange
    let invoked = Cell::new(false);
    let mut runner = stress::CassieStressRunner::new(
        "tier1_hotpath_row_codec",
        performance_benchmarks::BenchmarkTier::Tier1,
    );
    let case = tier1_row_case(
        performance_benchmarks::FixtureClass::Kernel,
        0,
        stress::OperationUnit::Key,
    );

    // Act
    let panic = std::panic::catch_unwind(AssertUnwindSafe(|| {
        runner.measure_micro(case, || {
            invoked.set(true);
            1_usize
        });
    }));

    // Assert
    assert!(panic.is_err());
    assert!(!invoked.get());
}

#[test]
fn should_allow_same_owner_scale_fixture_identity_reuse() {
    // Arrange
    let mut tracker = stress::FixtureIdentityTracker::default();

    // Act
    let first = tracker.register("tier5_scaling_query", "100k", "query-disk-100k");
    let reopened = tracker.register("tier5_scaling_query", "100k", "query-disk-100k");

    // Assert
    assert_eq!(first, Ok(()));
    assert_eq!(reopened, Ok(()));
}

#[test]
fn should_reject_different_fixture_identity_for_same_owner_scale() {
    // Arrange
    let mut tracker = stress::FixtureIdentityTracker::default();
    tracker
        .register("tier5_scaling_query", "100k", "query-disk-100k")
        .expect("first fixture identity");

    // Act
    let error = tracker
        .register("tier5_scaling_query", "100k", "second-query-disk-100k")
        .expect_err("different owner-scale fixture identity must fail");

    // Assert
    assert!(error.contains("different fixture identities"));
}

#[test]
fn should_record_external_batch_elapsed_once() {
    // Arrange
    let elapsed = Duration::from_millis(250);
    let completed_operations = 500;

    // Act
    let recorded = stress::external_elapsed(elapsed, completed_operations);

    // Assert
    assert_eq!(recorded, elapsed);
}

#[test]
fn should_default_soak_duration_to_one_hour() {
    // Arrange
    let environment = None;
    let command_line = None;

    // Act
    let duration =
        stress::resolve_soak_duration(environment, command_line).expect("default soak duration");

    // Assert
    assert_eq!(duration.total, Duration::from_hours(1));
    assert_eq!(duration.source, "default");
}

#[test]
fn should_prefer_cli_soak_duration_over_environment() {
    // Arrange
    let environment = Some("120");
    let command_line = Some("30");

    // Act
    let duration =
        stress::resolve_soak_duration(environment, command_line).expect("CLI soak duration");

    // Assert
    assert_eq!(duration.total, Duration::from_secs(30));
    assert_eq!(duration.source, "cli");
}

#[test]
fn should_divide_tier6_duration_across_measured_samples() {
    // Arrange
    let total = Duration::from_hours(1);
    let measured_samples = 5;

    // Act
    let per_sample =
        stress::soak_sample_duration(total, measured_samples).expect("per-sample soak duration");

    // Assert
    assert_eq!(per_sample, Duration::from_mins(12));
}

#[test]
fn should_reject_zero_soak_duration() {
    // Arrange
    let zero_duration = Some("0");

    // Act
    let duration_error = stress::resolve_soak_duration(zero_duration, None)
        .expect_err("zero soak duration must fail");

    // Assert
    assert!(duration_error.contains("positive"));
}

#[test]
fn should_reject_shortened_tier6_duration_outside_smoke_profile() {
    // Arrange
    let shortened = Duration::from_secs(5);

    // Act
    let error = stress::validate_soak_duration_for_profile(shortened, false)
        .expect_err("shortened canonical endurance evidence must fail");

    // Assert
    assert!(error.contains("only valid with STRESS_PROFILE=smoke"));
}

#[test]
fn should_allow_shortened_tier6_duration_given_smoke_profile() {
    // Arrange
    let shortened = Duration::from_secs(5);

    // Act
    let result = stress::validate_soak_duration_for_profile(shortened, true);

    // Assert
    assert_eq!(result, Ok(()));
}

#[test]
fn should_reject_zero_measured_samples() {
    // Arrange
    let measured_samples = 0;

    // Act
    let sample_error = stress::soak_sample_duration(Duration::from_secs(1), measured_samples)
        .expect_err("zero measured samples must fail");

    // Assert
    assert!(sample_error.contains("samples"));
}

#[test]
fn should_require_observed_preflight_for_scaling_queries() {
    // Arrange
    let scenario =
        performance_benchmarks::benchmark_for_scenario("perf.scale.query.relational.10k")
            .expect("registered Tier 5 query");

    // Act
    let error = stress::validate_preflight_requirement(scenario, None)
        .expect_err("unobserved scaling query evidence must fail");

    // Assert
    assert!(error.contains("observed preflight"));
}

#[test]
fn should_map_vector_families_to_persisted_access_paths() {
    // Arrange
    let cases = [
        ("perf.vector.executor.100k", "vector_exact"),
        ("perf.vector.hnsw_persisted.100k", "hnsw"),
        ("perf.vector.ivfflat_persisted.100k", "ivfflat"),
    ];

    // Act
    let observed = cases.map(|(scenario_id, _)| {
        performance_benchmarks::benchmark_for_scenario(scenario_id)
            .expect("registered vector scenario")
            .expected_selected_access_path()
    });

    // Assert
    assert_eq!(
        observed,
        [Some("vector_exact"), Some("hnsw"), Some("ivfflat")]
    );
}

#[test]
fn should_reject_mislabeled_vector_preflight_before_measurement() {
    // Arrange
    let scenario =
        performance_benchmarks::benchmark_for_scenario("perf.vector.hnsw_persisted.100k")
            .expect("registered HNSW scenario");
    let preflight = stress::PreflightEvidence::new("collection_scan", "none");

    // Act
    let error = stress::validate_preflight_requirement(scenario, Some(&preflight))
        .expect_err("mislabeled vector preflight must fail");

    // Assert
    assert!(error.contains("perf.vector.hnsw_persisted.100k"));
    assert!(error.contains("hnsw"));
    assert!(error.contains("collection_scan"));
}

#[test]
fn should_scope_candidate_count_to_access_family() {
    // Arrange
    let delta = json!({
        "query": { "rows_returned_total": 91 },
        "search": {
            "candidate_count_total": 17
        },
        "vector": {
            "candidate_count_total": 73
        }
    });

    // Act
    let candidates = stress::scoped_candidate_count(&delta, "fulltext");

    // Assert
    assert_eq!(candidates, 17);
}

#[test]
fn should_ignore_unrelated_family_fallback_metrics() {
    // Arrange
    let delta = json!({
        "search": { "row_scan_fallback_total": 0 },
        "vector": { "hnsw_fallbacks": 4, "row_scan_fallback_total": 2 }
    });
    let current = json!({
        "search": { "last_fallback_reason": "" },
        "vector": { "last_fallback_reason": "unrelated_vector_fallback" }
    });

    // Act
    let fallback = stress::scoped_fallback_evidence(&delta, &current, "fulltext");

    // Assert
    assert_eq!(fallback.count, 0);
    assert_eq!(fallback.reason, "none");
}

#[test]
fn should_report_only_selected_family_fallback_reason() {
    // Arrange
    let delta = json!({
        "search": { "row_scan_fallback_total": 3 },
        "vector": { "hnsw_fallbacks": 9 }
    });
    let current = json!({
        "search": { "last_fallback_reason": "posting_generation_mismatch" },
        "vector": { "last_fallback_reason": "unrelated_vector_fallback" }
    });

    // Act
    let fallback = stress::scoped_fallback_evidence(&delta, &current, "fulltext");

    // Assert
    assert_eq!(fallback.count, 3);
    assert_eq!(fallback.reason, "posting_generation_mismatch");
}

#[test]
fn should_use_access_family_candidate_metrics_before_result_rows() {
    // Arrange
    let delta = json!({
        "query": { "rows_returned_total": 999 },
        "joins": { "left_input_rows_total": 30, "right_input_rows_total": 40 },
        "parallel_aggregation": { "rows": 80 },
        "graph": { "rows": 90 },
        "time_series": { "index_entries_scanned": 100, "rows": 50 },
        "read_paths": { "collection_scan_rows": 110, "ordered_rows": 120 },
        "search": { "candidate_count_total": 10 },
        "vector": { "candidate_count_total": 20 },
        "hybrid": { "candidate_count_total": 30 }
    });

    // Act
    let observed = [
        stress::scoped_candidate_count(&delta, "join"),
        stress::scoped_candidate_count(&delta, "worker_saturation"),
        stress::scoped_candidate_count(&delta, "graph"),
        stress::scoped_candidate_count(&delta, "time_series"),
        stress::scoped_candidate_count(&delta, "relational_index"),
        stress::scoped_candidate_count(&delta, "mixed_load"),
    ];

    // Assert
    assert_eq!(observed, [70, 80, 90, 100, 230, 290]);
}

#[test]
fn should_use_time_series_rows_when_no_index_entries_are_observed() {
    // Arrange
    let delta = json!({
        "query": { "rows_returned_total": 999 },
        "time_series": { "index_entries_scanned": 0, "rows": 50 }
    });

    // Act
    let candidates = stress::scoped_candidate_count(&delta, "time_series");

    // Assert
    assert_eq!(candidates, 50);
}

#[test]
fn should_accumulate_retrieval_setup_only_in_explicit_setup_sections() {
    // Arrange
    let source = include_str!("../benches/tier5_scaling_retrieval.rs");

    // Act
    let explicit_setup_sections = source.matches("accumulate_setup(").count();

    // Assert
    assert!(explicit_setup_sections > 1);
    assert!(!source.contains("setup_started.elapsed()"));
    assert!(source.contains("fn accumulate_setup<T>"));
    assert!(source.contains("prepare_fulltext_warm_state"));
    assert!(source.contains("evidenced(fulltext, *setup_time"));
    assert!(source.contains("evidenced(cases.ivf, *setup_time"));
    assert!(source.contains("ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES.to_string()"));
}

#[test]
fn should_construct_one_shared_fixture_for_tier3_query_owner() {
    // Arrange
    let source = include_str!("../benches/tier3_system_query.rs");

    // Act
    let shared_fixture_constructions = source.matches("workloads::tier3_query_context(").count();
    let obsolete_fixture_constructors = [
        "workloads::context(",
        "workloads::vectorized_join_context(",
        "workloads::graph_context(",
        "workloads::time_series_context(",
    ]
    .into_iter()
    .filter(|constructor| source.contains(constructor))
    .collect::<Vec<_>>();

    // Assert
    assert_eq!(shared_fixture_constructions, 1);
    assert!(obsolete_fixture_constructors.is_empty());
    assert!(source.contains("prepare_tier3_query_domains"));
}

#[test]
fn should_clean_up_shared_tier3_query_fixture() {
    // Arrange
    let source = include_str!("../benches/tier3_system_query.rs");

    // Act
    let shuts_down_cassie = source.contains("context.cassie.shutdown()");
    let drops_context = source.contains("drop(context)");
    let finishes_runner = source.contains("runner.finish()");
    let removes_data_dir = source.contains("std::fs::remove_dir_all(&data_dir)");
    let removes_marker_file = source.contains("std::fs::remove_file(&data_dir)");
    let verifies_cleanup = source.contains("assert!(!data_dir.exists()");
    let finish_position = source.rfind("runner.finish()").expect("runner finish");
    let cleanup_position = source
        .find("std::fs::remove_dir_all(&data_dir)")
        .expect("fixture cleanup");

    // Assert
    assert!(shuts_down_cassie);
    assert!(drops_context);
    assert!(finishes_runner);
    assert!(removes_data_dir);
    assert!(removes_marker_file);
    assert!(verifies_cleanup);
    assert!(finish_position < cleanup_position);
}

#[test]
fn should_gate_scaling_projection_work_on_production_metric_names() {
    // Arrange
    let source = include_str!("../benches/support/workloads/scaling_legacy.rs");

    // Act
    let records_refresh = source.contains("materialized_refreshes");
    let records_verification = source.contains("integrity_verifications");

    // Assert
    assert!(records_refresh);
    assert!(records_verification);
}

#[test]
fn should_normalize_scaling_lifecycle_commands_by_source_rows() {
    // Arrange
    let workflow_scenarios = [
        "perf.rebuild.refresh.10k",
        "perf.rebuild.refresh.100k",
        "perf.scale.rebuild.refresh.250k",
        "perf.time_series.retention.100k",
        "perf.time_series.rollup_refresh.100k",
        "perf.verification.full.100k",
    ];

    // Act
    let operation_units = workflow_scenarios.map(|scenario_id| {
        performance_benchmarks::benchmark_for_scenario(scenario_id)
            .expect("registered Tier 5 lifecycle scenario")
            .operation_unit
    });
    let replay_unit =
        performance_benchmarks::benchmark_for_scenario("perf.replay.lag_catchup.100k")
            .expect("registered Tier 5 replay scenario")
            .operation_unit;

    // Assert
    assert_eq!(operation_units, ["source_row"; 6]);
    assert_eq!(replay_unit, "event");
    let owner_source = include_str!("../benches/tier5_scaling_lifecycle.rs");
    assert!(owner_source.contains("let source_rows = u64::try_from(rows)"));
    assert_eq!(owner_source.matches("source_rows,").count(), 6);
}

#[test]
fn should_prepare_time_series_mutations_outside_lifecycle_measurement() {
    // Arrange
    let setup_source = include_str!("../benches/support/workloads/scaling_legacy.rs");
    let timed_source = include_str!("../benches/support/workloads/system.rs");

    // Act
    let retention_body = timed_source
        .split_once("pub fn time_series_retention_enforcement")
        .expect("retention workload")
        .1
        .split_once("pub fn time_series_rollup_refresh")
        .expect("end of retention workload")
        .0;
    let rollup_body = timed_source
        .split_once("pub fn time_series_rollup_refresh")
        .expect("rollup workload")
        .1
        .split_once("pub fn timed_ingest_document")
        .expect("end of rollup workload")
        .0;

    // Assert
    assert!(setup_source.contains("ts-retention-expired-sentinel"));
    assert!(!retention_body.contains("put_documents"));
    assert!(!rollup_body.contains("put_documents"));
    assert!(retention_body.contains("\"enforcements\""));
    assert!(retention_body.contains("\"errors\""));
    assert!(rollup_body.contains("\"refreshes\""));
}

#[test]
fn should_prepare_projection_replay_inputs_before_measurement() {
    // Arrange
    let owner_source = include_str!("../benches/tier5_scaling_lifecycle.rs");
    let workload_source = include_str!("../benches/support/workloads/scaling.rs");

    // Act
    let setup_position = owner_source
        .find("prepare_isolated_projection_replay_batches")
        .expect("replay input setup");
    let measurement_position = owner_source
        .find("runner.measure_batch(")
        .expect("replay measurement");
    let timed_replay_body = workload_source
        .split_once("pub fn isolated_projection_replay(")
        .expect("timed replay function")
        .1
        .split_once("pub fn drop_vector_index")
        .expect("end of timed replay function")
        .0;

    // Assert
    assert!(setup_position < measurement_position);
    assert!(owner_source.contains(".take_next()"));
    assert!(owner_source.contains("PROJECTION_REPLAY_EVENTS_PER_BATCH"));
    assert!(!timed_replay_body.contains("ProjectionReplayEvent"));
    assert!(!timed_replay_body.contains("scale-replay-event"));
}

#[test]
fn should_construct_one_shared_fixture_given_tier2_ingest_cases() {
    // Arrange
    let owner_source = include_str!("../benches/tier2_subsystem_ingest.rs");

    // Act
    let constructor_count = owner_source.matches("ProjectionBatchFixture::new(").count();
    let setup_position = owner_source
        .find("ProjectionBatchFixture::new(")
        .expect("shared projection fixture setup");
    let filter_position = owner_source
        .find("write_enabled || replay_enabled")
        .expect("lazy shared fixture guard");
    let evidence_uses = owner_source
        .matches("runtime_evidence(fixture.cassie())")
        .count();
    let identity_uses = owner_source.matches("fixture.fixture_identity()").count();

    // Assert
    assert_eq!(constructor_count, 1);
    assert!(filter_position < setup_position);
    assert_eq!(evidence_uses, 2);
    assert_eq!(identity_uses, 1);
    assert!(!owner_source.contains("ProjectionBatchFixture::new_write"));
    assert!(!owner_source.contains("ProjectionBatchFixture::new_replay"));
}

#[test]
fn should_normalize_preserved_analytical_queries_by_result_rows() {
    // Arrange
    let scenario_ids = [
        "perf.core_read.recursive_cte.100k",
        "perf.core_read.window_frames.100k",
    ];
    let owner_source = include_str!("../benches/tier5_scaling_query.rs");

    // Act
    let operation_units = scenario_ids.map(|scenario_id| {
        performance_benchmarks::benchmark_for_scenario(scenario_id)
            .expect("registered preserved analytical scenario")
            .operation_unit
    });

    // Assert
    assert_eq!(operation_units, ["result_row"; 2]);
    assert!(owner_source.contains("recursive_cte_result_rows(UPPER_BOUND)"));
    assert!(owner_source.contains("u64::try_from(expected_rows)"));
    assert!(owner_source.contains("u64::try_from(EXPECTED_ROWS)"));
}

#[test]
fn should_label_dense_join_algorithm_selection_profile() {
    // Arrange
    let owner_source = include_str!("../benches/tier5_scaling_query.rs");
    let contract = include_str!("../docs/performance-contracts.md");

    // Act
    let artifact_profile_is_declared = owner_source
        .contains("case.metadata(\"benchmark_resource_profile\", \"dense_stream_selection_4k\")");
    let contract_names_profile =
        contract.contains("`benchmark_resource_profile=dense_stream_selection_4k`");

    // Assert
    assert!(artifact_profile_is_declared);
    assert!(contract_names_profile);
    assert!(contract.contains("4 KiB algorithm-selection profile"));
}
