use cassie::benchmark::{
    ExecutorKernel, PgwireParameterBindingKernel, RowCodecKernel, RowKeyKernel,
};
use cassie::types::Value;

#[path = "../benches/support/workloads.rs"]
mod workloads;

#[test]
fn should_prepare_registered_hotpath_fixtures_given_closed_workload_names() {
    // Arrange
    let registered_workloads = [
        "row_encode_decode",
        "key_encode_decode",
        "batch_filter",
        "batch_projection",
        "value_comparison",
        "tokenization",
        "row_to_pgwire_encoding",
        "predicate_evaluation",
        "top_k_heap_maintenance",
        "cosine_distance",
        "dot_product",
        "l2_distance",
        "bm25_scoring",
    ];

    // Act
    let results = registered_workloads.map(workloads::prepare_hotpath);

    // Assert
    assert!(results.into_iter().all(|result| result.is_ok()));
}

#[test]
fn should_reject_unregistered_hotpath_fixture_given_unknown_workload_name() {
    // Arrange
    let workload = "query_parameter_binding";

    // Act
    let result = workloads::prepare_hotpath(workload);

    // Assert
    assert_eq!(result, Err("unknown Tier 1 hot-path workload"));
}

#[test]
fn should_round_trip_binary_row_given_production_codec_fixture_when_decoded() {
    // Arrange
    let kernel = RowCodecKernel::sample();

    // Act
    let encoded = kernel.encode();
    let decoded = kernel.decode(&encoded);

    // Assert
    assert!(encoded.starts_with(b"CRB2"));
    assert_eq!(&decoded, kernel.expected_row());
}

#[test]
fn should_round_trip_row_identity_given_production_key_fixture_when_decoded() {
    // Arrange
    let kernel = RowKeyKernel::for_row(7, "doc-1");
    let other_relation = RowKeyKernel::for_row(8, "doc-1");

    // Act
    let (encoded, decoded) = kernel.encode_decode();
    let (encoded_again, decoded_again) = kernel.encode_decode();
    let (other_encoded, _) = other_relation.encode_decode();

    // Assert
    assert_eq!(decoded, "doc-1");
    assert_eq!(decoded_again, "doc-1");
    assert_eq!(encoded, encoded_again);
    assert_ne!(encoded, other_encoded);
    assert!(!encoded.windows(2).any(|window| window == b"v2"));
}

#[test]
fn should_decode_typed_values_given_production_pgwire_bind_parameters() {
    // Arrange
    let kernel = PgwireParameterBindingKernel::with_parameters(4);

    // Act
    let decoded = kernel.decode();

    // Assert
    assert_eq!(
        decoded,
        vec![
            Value::Int64(42),
            Value::String("alpha".to_string()),
            Value::Bool(true),
            Value::Float64(3.5),
        ]
    );
}

#[test]
fn should_match_predicate_given_executor_fixture_when_evaluated() {
    // Arrange
    let kernel = ExecutorKernel::sample();

    // Act
    let predicate_matches = kernel.predicate_matches();

    // Assert
    assert!(predicate_matches);
}

#[test]
fn should_match_all_value_pairs_given_executor_fixture_when_compared() {
    // Arrange
    let kernel = ExecutorKernel::sample();

    // Act
    let comparison_matches = kernel.matching_value_comparisons();

    // Assert
    assert_eq!(comparison_matches, 8);
}

#[test]
fn should_filter_rows_given_executor_fixture_when_batch_kernel_runs() {
    // Arrange
    let kernel = ExecutorKernel::sample();

    // Act
    let matched = kernel.filter_batch();

    // Assert
    assert_eq!(matched, 23);
}

#[test]
fn should_project_selected_value_given_executor_fixture_when_projection_runs() {
    // Arrange
    let kernel = ExecutorKernel::sample();

    // Act
    let projected = kernel.project_row();

    // Assert
    assert_eq!(projected, vec![Value::String("alpha".to_string())]);
}

#[test]
fn should_keep_highest_scores_given_executor_fixture_when_top_k_runs() {
    // Arrange
    let kernel = ExecutorKernel::sample();

    // Act
    let scores = kernel.top_k_scores();

    // Assert
    assert_eq!(scores, vec![9, 7, 4]);
}

#[test]
fn should_report_candidates_separately_given_top_k_kernel_observation() {
    // Arrange
    workloads::prepare_hotpath("top_k_heap_maintenance").expect("registered top-k workload");

    // Act
    let observation = workloads::top_k_update();

    // Assert
    assert_eq!(observation.completed_operations(), 1);
    assert_eq!(observation.result_cardinality(), 3);
    assert_eq!(observation.candidate_count(), Some(6));
}

#[test]
fn should_count_observed_candidates_given_hnsw_subsystem_search() {
    // Arrange
    let fixture = workloads::VectorCandidateFixture::new(128);

    // Act
    let observation = fixture.hnsw();

    // Assert
    assert_eq!(
        observation.completed_operations(),
        observation
            .candidate_count()
            .expect("HNSW candidate evidence")
    );
    assert!(observation.result_cardinality() <= observation.completed_operations());
}

#[test]
fn should_require_vector_execution_count_for_every_scaling_path() {
    // Arrange
    let index_kinds = [None, Some("hnsw"), Some("ivfflat")];

    // Act
    let required = index_kinds.map(workloads::vector_execution_count_is_required);

    // Assert
    assert_eq!(required, [true, true, true]);
}

#[test]
fn should_record_projection_lifecycle_metrics_given_small_scaling_fixture() {
    // Arrange
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("benchmark lifecycle test runtime");
    let context = runtime
        .block_on(workloads::disk_context_with_temp_budget(
            "benchmark-lifecycle-metrics",
            16,
            workloads::ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES,
        ))
        .expect("small projection lifecycle fixture");
    workloads::prepare_projection_lifecycle(&context);
    let before = context.cassie.metrics();

    // Act
    let cardinality = runtime.block_on(workloads::projection_verify_existing(&context));
    let after = context.cassie.metrics();

    // Assert
    assert!(cardinality > 0);
    assert!(
        after["projections"]["integrity_verifications"]
            .as_u64()
            .unwrap_or_default()
            > before["projections"]["integrity_verifications"]
                .as_u64()
                .unwrap_or_default(),
        "projection metrics before={before} after={after}"
    );
    let data_dir = context.data_dir.clone();
    context.cassie.shutdown();
    drop(context);
    std::fs::remove_dir_all(data_dir).expect("clean up lifecycle metric fixture");
}

#[test]
fn should_share_bounded_projection_fixture_given_write_and_replay_samples() {
    // Arrange
    let runtime = workloads::runtime();
    let fixture = workloads::ProjectionBatchFixture::new(&runtime, 8);
    let write_cassie = fixture.cassie();
    let replay_cassie = fixture.cassie();

    // Act
    let first_write = fixture.write_batch();
    let first_written = first_write.completed_operations();
    first_write.finish_sample();
    let first_replay = fixture.replay_batch();
    let first_replayed = first_replay.result_cardinality();
    first_replay.finish_sample();
    let second_write = fixture.write_batch();
    let second_written = second_write.completed_operations();
    second_write.finish_sample();
    let second_replay = fixture.replay_batch();
    let second_replayed = second_replay.result_cardinality();
    second_replay.finish_sample();
    let retained_documents = write_cassie
        .midge
        .scan_documents("bench_documents")
        .expect("shared projection fixture should remain readable");

    // Assert
    assert!(std::sync::Arc::ptr_eq(&write_cassie, &replay_cassie));
    assert_eq!(first_written, 8);
    assert_eq!(first_replayed, 8);
    assert_eq!(second_written, 8);
    assert_eq!(second_replayed, 8);
    assert_eq!(retained_documents.len(), 8);
    assert_eq!(fixture.retained_fixture_rows(), 8);
    assert!(fixture
        .fixture_identity()
        .contains("tier2-subsystem-projection"));
}

#[test]
fn should_retain_at_most_2048_logical_rows_given_tier2_projection_fixture() {
    // Arrange
    let runtime = workloads::runtime();

    // Act
    let fixture = workloads::ProjectionBatchFixture::new(&runtime, 2_048);

    // Assert
    assert_eq!(fixture.retained_fixture_rows(), 2_048);
}

#[test]
fn should_record_exact_vector_metrics_given_tier3_query_shape() {
    // Arrange
    const SQL: &str = "SELECT id, vector_distance(embedding, $1) AS distance FROM bench_documents ORDER BY distance ASC LIMIT 20";
    workloads::configure_tier3_environment();
    let runtime = workloads::runtime();
    let context = runtime
        .block_on(workloads::tier3_query_context(
            "tier3-exact-vector-metrics-test",
            32,
        ))
        .expect("shared Tier 3 fixture");
    let params = || {
        vec![Value::Vector(cassie::types::Vector::new(vec![
            1.0, 0.0, 0.0,
        ]))]
    };
    let before = context.cassie.metrics();

    // Act
    let rows = workloads::execute_expected_query(&context, SQL, params(), 20);
    let after = context.cassie.metrics();

    // Assert
    assert_eq!(rows, 20);
    assert_eq!(after["vector"]["count"].as_u64(), Some(1));
    assert_eq!(after["vector"]["candidate_count_total"].as_u64(), Some(32));
    assert_eq!(after["vector"]["result_count_total"].as_u64(), Some(20));
    assert_eq!(after["vector"]["hnsw_executions"].as_u64(), Some(0));
    assert_eq!(after["vector"]["ivfflat_executions"].as_u64(), Some(0));
    assert_eq!(before["vector"]["count"].as_u64(), Some(0));
    let data_dir = context.data_dir.clone();
    context.cassie.shutdown();
    drop(context);
    std::fs::remove_dir_all(data_dir).expect("clean up exact-vector metric fixture");
}

#[test]
fn should_report_verified_vector_access_paths_given_persisted_state() {
    // Arrange
    const SQL: &str = "SELECT id, vector_distance(embedding, $1) AS distance FROM bench_documents ORDER BY distance ASC LIMIT 20";
    workloads::configure_tier3_environment();
    let runtime = workloads::runtime();
    let context = runtime
        .block_on(workloads::tier3_query_context(
            "tier3-vector-access-evidence-test",
            32,
        ))
        .expect("shared Tier 3 fixture");
    let params = || {
        vec![Value::Vector(cassie::types::Vector::new(vec![
            1.0, 0.0, 0.0,
        ]))]
    };

    // Act
    let exact = workloads::assert_vector_preflight(
        &context,
        SQL,
        params(),
        "collection=postgres.public.bench_documents",
        32,
        workloads::VectorAccessPath::Exact,
    );
    workloads::create_hnsw_index(&context);
    let hnsw = workloads::assert_vector_preflight(
        &context,
        SQL,
        params(),
        "collection=postgres.public.bench_documents",
        32,
        workloads::VectorAccessPath::Hnsw,
    );
    let incomplete_hnsw = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        workloads::assert_vector_preflight(
            &context,
            SQL,
            params(),
            "collection=postgres.public.bench_documents",
            31,
            workloads::VectorAccessPath::Hnsw,
        )
    }));
    let wrong_state = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        workloads::assert_vector_preflight(
            &context,
            SQL,
            params(),
            "collection=postgres.public.bench_documents",
            32,
            workloads::VectorAccessPath::IvfFlat,
        )
    }));
    workloads::drop_vector_index(&context);
    workloads::create_ivfflat_index(&context);
    let ivf = workloads::assert_vector_preflight(
        &context,
        SQL,
        params(),
        "collection=postgres.public.bench_documents",
        32,
        workloads::VectorAccessPath::IvfFlat,
    );

    // Assert
    assert_eq!(exact.selected_access_path, "vector_exact");
    assert_eq!(hnsw.selected_access_path, "hnsw");
    assert_eq!(ivf.selected_access_path, "ivfflat");
    assert!(incomplete_hnsw.is_err());
    assert!(wrong_state.is_err());
    let data_dir = context.data_dir.clone();
    context.cassie.shutdown();
    drop(context);
    std::fs::remove_dir_all(data_dir).expect("clean up vector access evidence fixture");
}

#[test]
fn should_bound_hnsw_hybrid_candidates_given_default_maximum() {
    // Arrange
    const FIXTURE_ROWS: usize = 4_096;
    const EXPECTED_CANDIDATE_BOUND: u64 = 20 * 64;
    const SQL: &str = "SELECT id, hybrid_score(search_score(body, $1), vector_score(embedding, $2)) AS score FROM bench_documents ORDER BY score DESC LIMIT 20";
    workloads::configure_tier3_environment();
    let runtime = workloads::runtime();
    let context = runtime
        .block_on(workloads::tier3_query_context(
            "tier3-hybrid-candidate-bound-test",
            FIXTURE_ROWS,
        ))
        .expect("hybrid candidate-bound fixture");
    workloads::create_hnsw_index(&context);
    let params = || {
        vec![
            Value::String("alpha".to_string()),
            Value::Vector(cassie::types::Vector::new(vec![1.0, 0.0, 0.0])),
        ]
    };
    let before = context.cassie.metrics();

    // Act
    let rows = workloads::execute_expected_query(&context, SQL, params(), 20);
    let after = context.cassie.metrics();

    // Assert
    let candidates = after["hybrid"]["candidate_count_total"]
        .as_u64()
        .unwrap_or_default()
        .saturating_sub(
            before["hybrid"]["candidate_count_total"]
                .as_u64()
                .unwrap_or_default(),
        );
    let candidate_row_fetches = after["hybrid"]["candidate_row_fetches_total"]
        .as_u64()
        .unwrap_or_default()
        .saturating_sub(
            before["hybrid"]["candidate_row_fetches_total"]
                .as_u64()
                .unwrap_or_default(),
        );
    assert_eq!(rows, 20);
    assert!(candidates > 0);
    assert!(candidate_row_fetches > 0);
    assert!(
        candidates <= EXPECTED_CANDIDATE_BOUND,
        "hybrid candidate count {candidates} exceeded {EXPECTED_CANDIDATE_BOUND}"
    );
    assert!(
        candidate_row_fetches <= EXPECTED_CANDIDATE_BOUND,
        "hybrid candidate row fetches {candidate_row_fetches} exceeded {EXPECTED_CANDIDATE_BOUND}"
    );
    assert_eq!(
        after["hybrid"]["prefilter_fallback_count_total"].as_u64(),
        before["hybrid"]["prefilter_fallback_count_total"].as_u64()
    );
    assert_eq!(
        after["hybrid"]["row_scan_fallback_total"].as_u64(),
        before["hybrid"]["row_scan_fallback_total"].as_u64()
    );
    let data_dir = context.data_dir.clone();
    context.cassie.shutdown();
    drop(context);
    std::fs::remove_dir_all(data_dir).expect("clean up hybrid candidate-bound fixture");
}

#[test]
fn should_record_hybrid_fallback_given_missing_ann_state() {
    // Arrange
    const SQL: &str = "SELECT id, hybrid_score(search_score(body, $1), vector_score(embedding, $2)) AS score FROM bench_documents ORDER BY score DESC LIMIT 20";
    workloads::configure_tier3_environment();
    let runtime = workloads::runtime();
    let context = runtime
        .block_on(workloads::tier3_query_context(
            "tier3-hybrid-missing-ann-test",
            64,
        ))
        .expect("hybrid missing-ANN fixture");
    let params = vec![
        Value::String("alpha".to_string()),
        Value::Vector(cassie::types::Vector::new(vec![1.0, 0.0, 0.0])),
    ];
    let before = context.cassie.metrics();

    // Act
    let rows = workloads::execute_expected_query(&context, SQL, params, 20);
    let after = context.cassie.metrics();

    // Assert
    assert_eq!(rows, 20);
    assert!(
        after["hybrid"]["prefilter_fallback_count_total"]
            .as_u64()
            .unwrap_or_default()
            > before["hybrid"]["prefilter_fallback_count_total"]
                .as_u64()
                .unwrap_or_default()
    );
    assert!(
        after["hybrid"]["row_scan_fallback_total"]
            .as_u64()
            .unwrap_or_default()
            > before["hybrid"]["row_scan_fallback_total"]
                .as_u64()
                .unwrap_or_default()
    );
    assert_eq!(
        after["hybrid"]["retrieval_fallback_reasons"]["missing-ann-state"].as_u64(),
        Some(1)
    );
    let data_dir = context.data_dir.clone();
    context.cassie.shutdown();
    drop(context);
    std::fs::remove_dir_all(data_dir).expect("clean up hybrid missing-ANN fixture");
}

#[test]
fn should_use_bucket_native_time_series_given_shared_tier3_fixture() {
    // Arrange
    const SQL: &str = "SELECT tenant, amount FROM bench_time_series_events WHERE event_at >= $1 AND event_at < $2 ORDER BY event_at LIMIT 512";
    workloads::configure_tier3_environment();
    let runtime = workloads::runtime();
    let context = runtime
        .block_on(workloads::tier3_query_context(
            "tier3-shared-time-series-test",
            16,
        ))
        .expect("shared Tier 3 fixture");
    workloads::prepare_tier3_query_domains(
        &context,
        16,
        workloads::Tier3QueryDomains {
            join: false,
            graph: false,
            time_series: true,
        },
    )
    .expect("shared Tier 3 time-series domain");
    let params = || {
        vec![
            Value::String("2026-01-09T00:00:00Z".to_string()),
            Value::String("2026-01-10T00:00:00Z".to_string()),
        ]
    };
    workloads::assert_explain_contains(
        &context,
        SQL,
        params(),
        "time_series_storage=bucket-native-v1",
    );
    let before = context.cassie.metrics();

    // Act
    let rows = workloads::execute_expected_query(&context, SQL, params(), 16);
    let after = context.cassie.metrics();

    // Assert
    assert_eq!(rows, 16);
    assert!(
        after["time_series"]["bucket_native_hits"]
            .as_u64()
            .unwrap_or_default()
            > before["time_series"]["bucket_native_hits"]
                .as_u64()
                .unwrap_or_default(),
        "time-series metrics before={before} after={after}"
    );
    assert_eq!(
        after["time_series"]["fallback_scans"].as_u64(),
        before["time_series"]["fallback_scans"].as_u64(),
        "time-series fixture must not fall back to row-backed reads"
    );
    let data_dir = context.data_dir.clone();
    context.cassie.shutdown();
    drop(context);
    std::fs::remove_dir_all(data_dir).expect("clean up shared Tier 3 test fixture");
}
