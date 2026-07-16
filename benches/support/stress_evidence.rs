use std::sync::{Arc, Mutex};

use cassie::app::Cassie;
use cassie::runtime::RuntimeState;
use cntryl_stress::StressContext;

use crate::performance_benchmarks::{PerformanceBenchmarkScenario, ResultCachePolicy};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreflightEvidence {
    selected_access_path: String,
    fallback_reason: String,
}

impl PreflightEvidence {
    /// Creates evidence observed by an untimed preflight plan inspection.
    ///
    /// # Panics
    ///
    /// Panics when either observed value is empty.
    pub fn new(
        selected_access_path: impl Into<String>,
        fallback_reason: impl Into<String>,
    ) -> Self {
        let selected_access_path = selected_access_path.into();
        let fallback_reason = fallback_reason.into();
        assert!(
            !selected_access_path.is_empty(),
            "preflight selected access path must not be empty"
        );
        assert!(
            !fallback_reason.is_empty(),
            "preflight fallback reason must not be empty"
        );
        Self {
            selected_access_path,
            fallback_reason,
        }
    }

    #[must_use]
    pub fn selected_access_path(&self) -> &str {
        &self.selected_access_path
    }

    #[must_use]
    pub fn fallback_reason(&self) -> &str {
        &self.fallback_reason
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedFallbackEvidence {
    pub count: u64,
    pub reason: String,
}

#[derive(Clone)]
pub struct RuntimeEvidenceSource {
    source: RuntimeMetricsSource,
    previous: Arc<Mutex<serde_json::Value>>,
}

#[derive(Clone)]
enum RuntimeMetricsSource {
    Cassie(Arc<Cassie>),
    Runtime(Arc<RuntimeState>),
}

impl RuntimeEvidenceSource {
    pub fn new(cassie: Arc<Cassie>) -> Self {
        Self::from_source(RuntimeMetricsSource::Cassie(cassie))
    }

    pub fn from_runtime(runtime: Arc<RuntimeState>) -> Self {
        Self::from_source(RuntimeMetricsSource::Runtime(runtime))
    }

    fn from_source(source: RuntimeMetricsSource) -> Self {
        let previous = source.snapshot();
        Self {
            source,
            previous: Arc::new(Mutex::new(previous)),
        }
    }

    pub fn record(
        &self,
        context: &mut StressContext,
        scenario: &PerformanceBenchmarkScenario,
        preflight: Option<&PreflightEvidence>,
        result_cardinality: u64,
        observed_candidate_count: Option<u64>,
        observed_peak_query_memory_bytes: Option<u64>,
    ) {
        let current = self.source.snapshot();
        let mut previous = self.previous.lock().expect("benchmark evidence snapshot");
        let delta = numeric_delta(&current, &previous);
        *previous = current.clone();

        let storage_reads = storage_reads(&delta);
        let candidate_count = observed_candidate_count
            .unwrap_or_else(|| scoped_candidate_count(&delta, scenario.access_family));
        let peak_query_memory_bytes = observed_peak_query_memory_bytes
            .unwrap_or_else(|| pointer_u64(&current, "/query/peak_accounted_memory_bytes"));
        let execution_result_cache_hits = pointer_u64(&delta, "/execution_result_cache/hits");
        let runtime_fallback = scoped_fallback_evidence(&delta, &current, scenario.access_family);
        let (fallback_reason, fallback_evidence_source) = if runtime_fallback.count > 0 {
            (runtime_fallback.reason.as_str(), "runtime_metrics")
        } else if let Some(preflight) = preflight {
            (preflight.fallback_reason(), "preflight")
        } else {
            ("none", "runtime_metrics")
        };
        let leaked_active_operator_workers =
            pointer_u64(&current, "/runtime/active_operator_workers");
        let configured_worker_count = scenario.worker_count.map_or(0, u16::from);
        let (selected_access_path, access_path_evidence_source) = preflight
            .map_or(("not_applicable", "not_applicable"), |evidence| {
                (evidence.selected_access_path(), "preflight")
            });

        context.metadata("result_cardinality", result_cardinality);
        context.metadata("selected_access_path", selected_access_path);
        context.metadata("access_path_evidence_source", access_path_evidence_source);
        context.metadata("storage_reads", storage_reads);
        context.metadata("candidate_count", candidate_count);
        context.metadata("peak_query_memory_bytes", peak_query_memory_bytes);
        context.metadata("execution_result_cache_hits", execution_result_cache_hits);
        context.metadata("worker_count", configured_worker_count);
        context.metadata("configured_worker_count", configured_worker_count);
        context.metadata(
            "leaked_active_operator_workers",
            leaked_active_operator_workers,
        );
        context.metadata("worker_leak_evidence_source", "runtime_metrics");
        context.metadata("fallback_reason", fallback_reason);
        context.metadata("fallback_evidence_source", fallback_evidence_source);
        context.metadata(
            "runtime_metrics_delta",
            serde_json::json!({
                "storage_reads": storage_reads,
                "candidate_count": candidate_count,
                "peak_query_memory_bytes": peak_query_memory_bytes,
                "execution_result_cache_hits": execution_result_cache_hits,
                "fallback_count": runtime_fallback.count,
                "fallback_reason": fallback_reason,
                "configured_worker_count": configured_worker_count,
                "leaked_active_operator_workers": leaked_active_operator_workers,
            }),
        );
        assert_eq!(
            leaked_active_operator_workers, 0,
            "benchmark sample leaked active operator workers"
        );
    }
}

impl RuntimeMetricsSource {
    fn snapshot(&self) -> serde_json::Value {
        match self {
            Self::Cassie(cassie) => cassie.metrics(),
            Self::Runtime(runtime) => serde_json::to_value(runtime.snapshot())
                .expect("serialize benchmark runtime evidence"),
        }
    }
}

pub fn record_without_runtime(
    context: &mut StressContext,
    scenario: &PerformanceBenchmarkScenario,
    preflight: Option<&PreflightEvidence>,
    result_cardinality: u64,
    observed_candidate_count: Option<u64>,
    observed_peak_query_memory_bytes: Option<u64>,
) {
    assert_eq!(
        scenario.result_cache_policy,
        ResultCachePolicy::Disabled,
        "the dedicated result-cache benchmark requires observed runtime evidence"
    );
    let configured_worker_count = scenario.worker_count.map_or(0, u16::from);
    let (selected_access_path, fallback_reason, evidence_source) = preflight.map_or(
        ("not_applicable", "not_applicable", "not_applicable"),
        |evidence| {
            (
                evidence.selected_access_path(),
                evidence.fallback_reason(),
                "preflight",
            )
        },
    );
    context.metadata("result_cardinality", result_cardinality);
    context.metadata("selected_access_path", selected_access_path);
    context.metadata("access_path_evidence_source", evidence_source);
    context.metadata("storage_reads", 0);
    let candidate_count = observed_candidate_count.unwrap_or(0);
    context.metadata("candidate_count", candidate_count);
    context.metadata(
        "peak_query_memory_bytes",
        observed_peak_query_memory_bytes.unwrap_or(0),
    );
    context.metadata("execution_result_cache_hits", 0);
    context.metadata("worker_count", configured_worker_count);
    context.metadata("configured_worker_count", configured_worker_count);
    context.metadata("leaked_active_operator_workers", 0);
    context.metadata("worker_leak_evidence_source", "not_applicable");
    context.metadata("fallback_reason", fallback_reason);
    context.metadata("fallback_evidence_source", evidence_source);
}

/// Validates that a gate row has evidence obtained before measurement.
///
/// # Errors
///
/// Returns an error when an applicable query row has no preflight evidence.
pub fn validate_preflight_requirement(
    scenario: &PerformanceBenchmarkScenario,
    preflight: Option<&PreflightEvidence>,
) -> Result<(), String> {
    if scenario.requires_observed_query_evidence() && preflight.is_none() {
        return Err(format!(
            "gate scenario {} requires observed preflight access-path and fallback evidence",
            scenario.scenario_id
        ));
    }
    if let (Some(expected), Some(preflight)) = (scenario.expected_selected_access_path(), preflight)
    {
        let observed = preflight.selected_access_path();
        if observed != expected {
            return Err(format!(
                "gate scenario {} selected access path mismatch: observed '{observed}', expected '{expected}'",
                scenario.scenario_id
            ));
        }
    }
    Ok(())
}

fn numeric_delta(current: &serde_json::Value, previous: &serde_json::Value) -> serde_json::Value {
    match current {
        serde_json::Value::Object(fields) => serde_json::Value::Object(
            fields
                .iter()
                .map(|(key, value)| {
                    let previous = previous.get(key).unwrap_or(&serde_json::Value::Null);
                    (key.clone(), numeric_delta(value, previous))
                })
                .collect(),
        ),
        serde_json::Value::Number(number) => {
            let current = number.as_u64().unwrap_or_default();
            let previous = previous.as_u64().unwrap_or_default();
            serde_json::Value::from(current.saturating_sub(previous))
        }
        _ => current.clone(),
    }
}

fn storage_reads(delta: &serde_json::Value) -> u64 {
    let retrieval_reads = [
        "/cardinality/reads",
        "/search/posting_reads_total",
        "/search/candidate_row_fetches_total",
        "/search/ann_reads_total",
        "/vector/posting_reads_total",
        "/vector/candidate_row_fetches_total",
        "/vector/ann_reads_total",
        "/hybrid/posting_reads_total",
        "/hybrid/candidate_row_fetches_total",
        "/hybrid/ann_reads_total",
    ]
    .into_iter()
    .map(|pointer| pointer_u64(delta, pointer))
    .sum::<u64>();
    retrieval_reads.saturating_add(storage_family_reads(&delta["storage"]))
}

fn storage_family_reads(value: &serde_json::Value) -> u64 {
    value.as_object().map_or(0, |families| {
        families
            .values()
            .map(|family| pointer_u64(family, "/reads"))
            .sum()
    })
}

#[must_use]
pub fn scoped_candidate_count(delta: &serde_json::Value, access_family: &str) -> u64 {
    if access_family.contains("mixed") {
        pointer_u64(delta, "/search/candidate_count_total")
            .saturating_add(pointer_u64(delta, "/vector/candidate_count_total"))
            .saturating_add(pointer_u64(delta, "/hybrid/candidate_count_total"))
            .saturating_add(pointer_u64(delta, "/read_paths/collection_scan_rows"))
            .saturating_add(pointer_u64(delta, "/read_paths/ordered_rows"))
    } else if access_family.contains("hybrid") {
        pointer_u64(delta, "/hybrid/candidate_count_total")
    } else if access_family.contains("vector") || access_family.contains("ann") {
        pointer_u64(delta, "/vector/candidate_count_total")
    } else if access_family.contains("fulltext") || access_family.contains("posting") {
        pointer_u64(delta, "/search/candidate_count_total")
    } else if access_family.contains("join") {
        pointer_u64(delta, "/joins/left_input_rows_total")
            .saturating_add(pointer_u64(delta, "/joins/right_input_rows_total"))
    } else if access_family.contains("worker") {
        pointer_u64(delta, "/parallel_aggregation/rows")
    } else if access_family.contains("graph") {
        pointer_u64(delta, "/graph/rows")
    } else if access_family.contains("time_series") {
        let index_entries = pointer_u64(delta, "/time_series/index_entries_scanned");
        if index_entries > 0 {
            index_entries
        } else {
            pointer_u64(delta, "/time_series/rows")
        }
    } else if access_family.contains("relational") {
        pointer_u64(delta, "/read_paths/collection_scan_rows")
            .saturating_add(pointer_u64(delta, "/read_paths/ordered_rows"))
    } else {
        pointer_u64(delta, "/query/rows_returned_total")
    }
}

const HYBRID_FALLBACK_COUNTS: &[&str] = &[
    "/hybrid/normalized_fallback_count_total",
    "/hybrid/prefilter_fallback_count_total",
    "/hybrid/row_scan_fallback_total",
];
const VECTOR_HNSW_FALLBACK_COUNTS: &[&str] = &[
    "/vector/hnsw_fallbacks",
    "/vector/prefilter_fallback_count_total",
    "/vector/row_scan_fallback_total",
];
const VECTOR_IVF_FALLBACK_COUNTS: &[&str] = &[
    "/vector/ivfflat_fallbacks",
    "/vector/prefilter_fallback_count_total",
    "/vector/row_scan_fallback_total",
];
const VECTOR_FALLBACK_COUNTS: &[&str] = &[
    "/vector/normalized_fallback_count_total",
    "/vector/prefilter_fallback_count_total",
    "/vector/row_scan_fallback_total",
];
const SEARCH_FALLBACK_COUNTS: &[&str] = &[
    "/search/normalized_fallback_count_total",
    "/search/prefilter_fallback_count_total",
    "/search/row_scan_fallback_total",
];
const JOIN_FALLBACK_COUNTS: &[&str] = &[
    "/joins/fallback_joins",
    "/joins/vectorized_fallbacks",
    "/joins/vectorized_spill_fallbacks",
];
const COLUMN_FALLBACK_COUNTS: &[&str] = &[
    "/column_batches/fallback_scans",
    "/column_batches/decode_fallbacks",
    "/aggregate_acceleration/decoded_fallback_segments",
    "/aggregate_acceleration/row_blob_fallbacks",
];
const WORKER_FALLBACK_COUNTS: &[&str] = &[
    "/parallel_scans/fallback_scans",
    "/parallel_scoring/fallback_scorings",
    "/parallel_aggregation/fallback_aggregations",
];
const PROJECTION_FALLBACK_COUNTS: &[&str] = &[
    "/projections/replay_errors",
    "/projections/mixed_execution_fallbacks",
    "/projections/rebuild_verification_failures",
];
const MIXED_FALLBACK_COUNTS: &[&str] = &[
    "/search/normalized_fallback_count_total",
    "/search/prefilter_fallback_count_total",
    "/search/row_scan_fallback_total",
    "/vector/normalized_fallback_count_total",
    "/vector/prefilter_fallback_count_total",
    "/vector/row_scan_fallback_total",
    "/vector/hnsw_fallbacks",
    "/vector/ivfflat_fallbacks",
    "/hybrid/normalized_fallback_count_total",
    "/hybrid/prefilter_fallback_count_total",
    "/hybrid/row_scan_fallback_total",
];
const VECTOR_REASON: &[&str] = &["/vector/last_fallback_reason"];
const SEARCH_REASON: &[&str] = &["/search/last_fallback_reason"];
const HYBRID_REASON: &[&str] = &["/hybrid/last_fallback_reason"];
const JOIN_REASONS: &[&str] = &[
    "/joins/last_fallback_reason",
    "/joins/last_vectorized_fallback_reason",
];
const COLUMN_REASON: &[&str] = &["/column_batches/last_fallback_reason"];
const TIME_SERIES_REASON: &[&str] = &["/time_series/last_fallback_reason"];
const WORKER_REASON: &[&str] = &["/parallel_aggregation/last_fallback_reason"];
const PROJECTION_REASON: &[&str] = &["/projections/last_fallback_reason"];
const MIXED_REASONS: &[&str] = &[
    "/search/last_fallback_reason",
    "/vector/last_fallback_reason",
    "/hybrid/last_fallback_reason",
];

#[must_use]
pub fn scoped_fallback_evidence(
    delta: &serde_json::Value,
    current: &serde_json::Value,
    access_family: &str,
) -> ScopedFallbackEvidence {
    let (count_pointers, reason_pointers) = fallback_scope(access_family);
    let count = count_pointers
        .iter()
        .map(|pointer| pointer_u64(delta, pointer))
        .sum();
    let reason = if count == 0 {
        "none".to_string()
    } else {
        reason_pointers
            .iter()
            .find_map(|pointer| {
                current
                    .pointer(pointer)
                    .and_then(serde_json::Value::as_str)
                    .filter(|reason| !reason.is_empty())
            })
            .unwrap_or("fallback_observed")
            .to_string()
    };
    ScopedFallbackEvidence { count, reason }
}

fn fallback_scope(access_family: &str) -> (&'static [&'static str], &'static [&'static str]) {
    if access_family.contains("mixed") {
        (MIXED_FALLBACK_COUNTS, MIXED_REASONS)
    } else if access_family.contains("hybrid") {
        (HYBRID_FALLBACK_COUNTS, HYBRID_REASON)
    } else if access_family.contains("vector_hnsw") {
        (VECTOR_HNSW_FALLBACK_COUNTS, VECTOR_REASON)
    } else if access_family.contains("vector_ivf") {
        (VECTOR_IVF_FALLBACK_COUNTS, VECTOR_REASON)
    } else if access_family.contains("vector") || access_family.contains("ann") {
        (VECTOR_FALLBACK_COUNTS, VECTOR_REASON)
    } else if access_family.contains("fulltext") || access_family.contains("posting") {
        (SEARCH_FALLBACK_COUNTS, SEARCH_REASON)
    } else if access_family.contains("join") {
        (JOIN_FALLBACK_COUNTS, JOIN_REASONS)
    } else if access_family.contains("column") {
        (COLUMN_FALLBACK_COUNTS, COLUMN_REASON)
    } else if access_family.contains("time_series") {
        (&["/time_series/fallback_scans"], TIME_SERIES_REASON)
    } else if access_family.contains("worker") {
        (WORKER_FALLBACK_COUNTS, WORKER_REASON)
    } else if access_family.contains("replay") || access_family.contains("rebuild") {
        (PROJECTION_FALLBACK_COUNTS, PROJECTION_REASON)
    } else {
        (&[], &[])
    }
}

fn pointer_u64(value: &serde_json::Value, pointer: &str) -> u64 {
    value
        .pointer(pointer)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default()
}
