//! Cassie-side runner: fixture ingest, index builds, measured queries and result rows.
//!
//! Index build, warmup and workload setup are timed or executed outside the measured
//! queries. Each (metric, algorithm, tuning point) configuration runs on its own copy of
//! the ingested data directory with a fresh Cassie instance, so its counters and peak
//! accounted query memory belong to that configuration alone.

use std::fmt::Write;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use cassie::app::{Cassie, CassieError, StorageRetryKind};
use cassie::config::{
    CassieRuntimeConfig, EmbeddingsRuntimeConfig, ExecutionResultCacheEnabled, LocalRuntimeConfig,
};
use cassie::types::Value;
use serde_json::{json, Value as Json};

use crate::embedding_workloads::ground_truth::{distance, recall_at_k, Metric, Truth};
use crate::embedding_workloads::manifest::{IndexTuning, Manifest};
use crate::embedding_workloads::product_selection::Configuration;
use crate::embedding_workloads::statistics::{mean, percentile};
use crate::loader::{Workload, SPLITS};

const TABLE: &str = "embedding_docs";
const WARMUP_QUERIES: usize = 5;
const INGEST_BATCH: usize = 1_000;
const INGEST_STALL_ATTEMPTS: u32 = 400;
const INGEST_STALL_STEP: Duration = Duration::from_millis(5);
const INGEST_STALL_MAX: Duration = Duration::from_millis(250);
const FILTERS: [(&str, Option<u32>); 4] = [
    ("unfiltered", None),
    ("t1", Some(1)),
    ("t10", Some(10)),
    ("t50", Some(50)),
];

pub struct RunOptions {
    pub tuning_points: usize,
    /// Splits to measure; a selection sweep runs `tuning` only.
    pub splits: Vec<String>,
    /// Measure every swept candidate instead of the selected points.
    pub sweep: bool,
    /// Metrics to run (`l2`, `cosine`); the manifest ground-truth metrics when empty.
    pub metrics: Vec<String>,
    /// Algorithms to run: `exact`, `hnsw` and `ivfflat`.
    pub algorithms: Vec<String>,
    /// Skip the exact configuration and the warmup: report recall and candidate counts only.
    pub recall_only: bool,
    pub query_limit: Option<usize>,
    pub query_range: Option<Range<usize>>,
    pub product_configuration: Option<Configuration>,
    pub work_dir: PathBuf,
    /// Accounted memory budget for one query; the 10 MiB default rejects exact scans of
    /// this size, so the runner raises it and records the value in the results.
    pub query_memory_budget_bytes: usize,
}

#[derive(Clone)]
pub struct IndexPlan {
    pub algorithm: &'static str,
    pub point: String,
    pub options: String,
}

fn runtime_config(dimensions: usize, query_memory_budget_bytes: usize) -> CassieRuntimeConfig {
    let mut config = CassieRuntimeConfig::from_env().expect("benchmark runtime config");
    config.embeddings = EmbeddingsRuntimeConfig::Local(LocalRuntimeConfig {
        model: "deterministic-bench".to_string(),
        dimensions,
    });
    config.limits.query_memory_budget_bytes = query_memory_budget_bytes;
    // Index builds at 100k documents exceed the 30 s default deadline on a shared machine.
    config.limits.query_timeout_ms = 0;
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    config
}

fn open(dir: &Path, dimensions: usize, memory_budget: usize) -> Result<Cassie, String> {
    let cassie =
        Cassie::new_with_data_dir_and_config(dir, runtime_config(dimensions, memory_budget))
            .map_err(|error| format!("open {}: {error}", dir.display()))?;
    cassie
        .startup()
        .map_err(|error| format!("startup {}: {error}", dir.display()))?;
    Ok(cassie)
}

fn run_sql(cassie: &Cassie, sql: &str) -> Result<cassie::executor::QueryResult, String> {
    cassie
        .execute_sql(&cassie.create_session("root", None), sql, Vec::new())
        .map_err(|error| format!("{}: {error}", sql.chars().take(160).collect::<String>()))
}

fn literal(vector: &[f32]) -> String {
    let parts: Vec<String> = vector.iter().map(f32::to_string).collect();
    format!("[{}]", parts.join(","))
}

fn dir_bytes(path: &Path) -> u64 {
    std::fs::read_dir(path).map_or(0, |entries| {
        entries
            .flatten()
            .map(|entry| match entry.metadata() {
                Ok(meta) if meta.is_dir() => dir_bytes(&entry.path()),
                Ok(meta) => meta.len(),
                Err(_) => 0,
            })
            .sum()
    })
}

fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|error| error.to_string())?;
    for entry in std::fs::read_dir(from)
        .map_err(|error| error.to_string())?
        .flatten()
    {
        if entry.file_name().to_string_lossy().starts_with(".midge") {
            continue; // leader and writer lock files belong to the closed instance
        }
        let target = to.join(entry.file_name());
        if entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_dir()
        {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

/// Loads one ingest batch. A 768-dimension load outruns memtable flushing, and Midge refuses
/// the next transaction with a write stall at admission, before applying it. The fresh loader
/// has no retry of its own, so the batch is resubmitted after a capped backoff.
fn put_fresh_batch(cassie: &Cassie, batch: &[(Option<String>, Json)]) -> Result<(), String> {
    for attempt in 1..=INGEST_STALL_ATTEMPTS {
        match cassie
            .midge
            .put_fresh_documents(TABLE, batch.to_vec())
            .map_err(CassieError::from)
        {
            Ok(_) => return Ok(()),
            Err(CassieError::StorageRetryable(error))
                if error.kind() == StorageRetryKind::WriteStall
                    && attempt < INGEST_STALL_ATTEMPTS =>
            {
                std::thread::sleep(
                    INGEST_STALL_STEP
                        .saturating_mul(attempt)
                        .min(INGEST_STALL_MAX),
                );
            }
            Err(error) => return Err(format!("ingest: {error}")),
        }
    }
    Err("ingest: write stall retries exhausted".to_string())
}

/// Ingests every document once into `<work>/base`; returns the directory and ingest time.
fn build_base(workload: &Workload, options: &RunOptions) -> Result<(PathBuf, f64), String> {
    let base = options
        .work_dir
        .join(format!("{}-{}-base", workload.model_key, workload.set));
    let _ = std::fs::remove_dir_all(&base);
    let started = Instant::now();
    let cassie = open(
        &base,
        workload.dimensions,
        options.query_memory_budget_bytes,
    )?;
    run_sql(
        &cassie,
        "CREATE EXTENSION IF NOT EXISTS vector VERSION '0.8.7'",
    )?;
    run_sql(
        &cassie,
        &format!(
            "CREATE TABLE {TABLE} (content TEXT, t1 INT, t10 INT, t50 INT, embedding VECTOR({}))",
            workload.dimensions
        ),
    )?;
    let total = workload.document_ids.len();
    for start in (0..total).step_by(INGEST_BATCH) {
        let batch: Vec<_> = (start..(start + INGEST_BATCH).min(total))
            .map(|ordinal| {
                let id = &workload.document_ids[ordinal];
                let flag = |percent| {
                    i32::from(crate::embedding_workloads::manifest::in_tenant(id, percent))
                };
                (
                    Some(ordinal.to_string()),
                    json!({
                        "content": ordinal.to_string(),
                        "t1": flag(1), "t10": flag(10), "t50": flag(50),
                        "embedding": workload.document(ordinal),
                    }),
                )
            })
            .collect();
        put_fresh_batch(&cassie, &batch)?;
    }
    // Fresh document loads require a table without secondary indexes, so the tenant
    // indexes are built after the load.
    for (name, _) in FILTERS.iter().skip(1) {
        run_sql(
            &cassie,
            &format!("CREATE INDEX {TABLE}_{name} ON {TABLE} USING btree ({name})"),
        )?;
    }
    cassie.shutdown();
    drop(cassie);
    Ok((base, started.elapsed().as_secs_f64()))
}

/// The index tuning points for an algorithm at `rows` documents, from the manifest.
///
/// `points` limits the selected points per index; `sweep` returns every swept candidate.
#[must_use]
pub fn index_plans(
    rows: usize,
    points: usize,
    tuning: &IndexTuning,
    sweep: bool,
) -> Vec<IndexPlan> {
    let lists = (1..=rows.max(1))
        .find(|n| n * n >= rows)
        .unwrap_or(1)
        .max(4);
    let sample = rows.min(20 * lists).max(lists);
    let pick = |selected: &[usize], candidates: &[usize]| -> Vec<usize> {
        if sweep {
            candidates.to_vec()
        } else {
            selected.iter().copied().take(points.max(1)).collect()
        }
    };
    let hnsw = &tuning.hnsw;
    let ivf = &tuning.ivfflat;
    let mut plans = Vec::new();
    for search in pick(&hnsw.ef_search_points, &hnsw.ef_search_candidates) {
        let (m, build) = (hnsw.m, hnsw.ef_construction);
        plans.push(IndexPlan {
            algorithm: "hnsw",
            point: format!("m{m}_efc{build}_efs{search}"),
            options: format!(
                "index_type = hnsw, m = {m}, ef_construction = {build}, ef_search = {search}"
            ),
        });
    }
    for permille in pick(&ivf.probe_permille_points, &ivf.probe_permille_candidates) {
        let probes = (lists * permille).div_ceil(1000).max(1);
        plans.push(IndexPlan {
            algorithm: "ivfflat",
            point: format!("lists{lists}_probes{probes}"),
            options: format!(
                "index_type = ivfflat, lists = {lists}, probes = {probes}, training_sample_size = {sample}, training_seed = 42"
            ),
        });
    }
    plans
}

fn counter(metrics: &Json, section: &str, name: &str) -> u64 {
    metrics[section][name].as_u64().unwrap_or(0)
}

#[derive(Default, Clone)]
struct Counters {
    ann_executions: u64,
    algorithm_fallbacks: u64,
    filtered_ann: u64,
    ann_candidates: u64,
    row_fetches: u64,
    exact_scan_rows: u64,
    reasons: std::collections::BTreeMap<String, u64>,
}

impl Counters {
    fn read(cassie: &Cassie) -> Self {
        let metrics = cassie.metrics();
        let filtered = &metrics["vector_filtered"];
        let reasons =
            filtered["fallback_reasons"]
                .as_object()
                .map_or_else(Default::default, |map| {
                    map.iter()
                        .map(|(k, v)| (k.clone(), v.as_u64().unwrap_or(0)))
                        .collect()
                });
        Self {
            ann_executions: counter(&metrics, "vector", "hnsw_executions")
                + counter(&metrics, "vector", "ivfflat_executions"),
            algorithm_fallbacks: counter(&metrics, "vector", "hnsw_fallbacks")
                + counter(&metrics, "vector", "ivfflat_fallbacks"),
            filtered_ann: filtered["paths"]["standalone.filtered-ann"]
                .as_u64()
                .unwrap_or(0),
            ann_candidates: counter(&metrics, "vector_filtered", "ann_candidates_total"),
            row_fetches: counter(&metrics, "vector", "candidate_row_fetches_total"),
            exact_scan_rows: counter(&metrics, "vector", "prefilter_input_candidate_count_total"),
            reasons,
        }
    }

    fn reason_delta(&self, before: &Self) -> Vec<String> {
        self.reasons
            .iter()
            .filter(|(reason, count)| **count > before.reasons.get(*reason).copied().unwrap_or(0))
            .map(|(reason, _)| reason.clone())
            .collect()
    }
}

struct QueryOutcome {
    query: usize,
    micros: f64,
    recall: f64,
    served_by_ann: bool,
    candidates: u64,
    reasons: Vec<String>,
}

fn query_sql(workload: &Workload, query: usize, metric: Metric, filter: Option<&str>) -> String {
    let predicate = filter.map_or_else(String::new, |column| format!(" WHERE {column} = 1"));
    format!(
        "SELECT content, embedding {op} '{lit}' AS distance FROM {TABLE}{predicate} \
         ORDER BY embedding {op} '{lit}' LIMIT 20",
        op = metric.operator(),
        lit = literal(workload.query(query))
    )
}

fn returned_rows(result: &cassie::executor::QueryResult) -> Vec<usize> {
    result
        .rows()
        .iter()
        .filter_map(|row| match &row[0] {
            Value::String(id) => id.parse().ok(),
            Value::Int64(id) => usize::try_from(*id).ok(),
            _ => None,
        })
        .collect()
}

struct Measurement<'a> {
    cassie: &'a Cassie,
    workload: &'a Workload,
    metric: Metric,
    indexed: bool,
    recall_only: bool,
}

impl Measurement<'_> {
    fn run(
        &self,
        queries: &[usize],
        truths: &[Truth],
        filter: Option<&str>,
    ) -> Result<Vec<QueryOutcome>, String> {
        let mut outcomes = Vec::with_capacity(queries.len());
        for (&query, truth) in queries.iter().zip(truths) {
            let sql = query_sql(self.workload, query, self.metric, filter);
            let before = Counters::read(self.cassie);
            let started = Instant::now();
            let result = run_sql(self.cassie, &sql)?;
            let elapsed = started.elapsed();
            let after = Counters::read(self.cassie);
            let rows = returned_rows(&result);
            let query_vector = self.workload.query(query);
            let recall = recall_at_k(truth, &rows, |row| {
                distance(self.metric, self.workload.document(row), query_vector)
            });
            let used_ann = after.ann_executions > before.ann_executions
                || after.filtered_ann > before.filtered_ann;
            let fell_back = after.algorithm_fallbacks > before.algorithm_fallbacks;
            outcomes.push(QueryOutcome {
                query,
                micros: elapsed.as_secs_f64() * 1e6,
                recall,
                served_by_ann: self.indexed && used_ann && !fell_back,
                candidates: (after.ann_candidates - before.ann_candidates)
                    + (after.row_fetches - before.row_fetches)
                    + (after.exact_scan_rows - before.exact_scan_rows),
                reasons: after.reason_delta(&before),
            });
        }
        Ok(outcomes)
    }
}

struct RowContext<'a> {
    workload: &'a Workload,
    metric: Metric,
    algorithm: &'a str,
    point: &'a str,
    options: &'a str,
    build_ms: f64,
    index_artifact_bytes: u64,
    index_dir_growth_bytes: u64,
    peak_memory_bytes: u64,
}

/// One (filter, split) cell of a configuration.
struct Cell {
    filter: &'static str,
    selectivity: Option<u32>,
    subset_size: usize,
    split: String,
    plan: String,
    outcomes: Vec<QueryOutcome>,
}

fn lossy(value: usize) -> f64 {
    f64::from(u32::try_from(value).unwrap_or(u32::MAX))
}

fn summarize(context: &RowContext<'_>, cell: &Cell, kind: &str, group: &[&QueryOutcome]) -> Json {
    let micros: Vec<f64> = group.iter().map(|o| o.micros).collect();
    let recalls: Vec<f64> = group.iter().map(|o| o.recall).collect();
    let total_seconds: f64 = micros.iter().sum::<f64>() / 1e6;
    let candidates: Vec<f64> = group
        .iter()
        .map(|o| lossy(usize::try_from(o.candidates).unwrap_or(usize::MAX)))
        .collect();
    let mut reasons = std::collections::BTreeMap::<String, u64>::new();
    for reason in group.iter().flat_map(|outcome| &outcome.reasons) {
        *reasons.entry(reason.clone()).or_default() += 1;
    }
    let count = lossy(group.len());
    let total = lossy(cell.outcomes.len());
    json!({
        "model": context.workload.model_key,
        "set": context.workload.set,
        "documents": context.workload.document_ids.len(),
        "dimensions": context.workload.dimensions,
        "metric": context.metric.name(),
        "algorithm": context.algorithm,
        "tuning_point": context.point,
        "index_options": context.options,
        "filter": cell.filter,
        "selectivity_percent": cell.selectivity,
        "subset_size": cell.subset_size,
        "split": cell.split,
        "row_kind": kind,
        "counts_as_ann_evidence": kind == "ann",
        "queries": group.len(),
        "queries_in_group": cell.outcomes.len(),
        "fallback_rate": if kind == "ann" { 1.0 - count / total } else { count / total },
        "recall_at_20_mean": mean(&recalls),
        "recall_at_20_min": recalls.iter().copied().fold(f64::INFINITY, f64::min),
        "latency_p50_us": percentile(&micros, 50),
        "latency_p95_us": percentile(&micros, 95),
        "qps": if total_seconds > 0.0 { count / total_seconds } else { 0.0 },
        "candidates_mean": mean(&candidates),
        "plan": cell.plan,
        "fallback_reasons": reasons,
        "build_ms": context.build_ms,
        "index_artifact_bytes": context.index_artifact_bytes,
        "index_dir_growth_bytes": context.index_dir_growth_bytes,
        "peak_query_memory_bytes_configuration_scope": context.peak_memory_bytes,
        "per_query": group.iter().map(|o| json!({
            "query_id": context.workload.query_ids[o.query],
            "recall_at_20": o.recall,
            "latency_us": o.micros,
            "candidates": o.candidates,
            "reasons": o.reasons,
        })).collect::<Vec<_>>(),
    })
}

fn explain(cassie: &Cassie, sql: &str) -> String {
    run_sql(cassie, &format!("EXPLAIN {sql}"))
        .ok()
        .and_then(|result| match &result.rows().first()?[0] {
            Value::String(plan) => Some(plan.replace('\n', " | ")),
            _ => None,
        })
        .unwrap_or_else(|| "explain-unavailable".to_string())
}

struct IndexStats {
    options: String,
    build_ms: f64,
    artifact_bytes: u64,
}

/// Builds the vector index (timed, outside every measured query). Falls back to an L2
/// index, noted in the options text, when the algorithm does not serve `metric`.
fn build_index(
    cassie: &Cassie,
    plan: Option<&IndexPlan>,
    metric: Metric,
) -> Result<IndexStats, String> {
    let Some(plan) = plan else {
        return Ok(IndexStats {
            options: "none".to_string(),
            build_ms: 0.0,
            artifact_bytes: 0,
        });
    };
    let mut options = plan.options.clone();
    let sql = |metric_name: &str| {
        format!(
            "CREATE INDEX {TABLE}_vec ON {TABLE} USING vector (embedding) WITH (source_field = content, metric = {metric_name}, {})",
            plan.options
        )
    };
    let started = Instant::now();
    if let Err(error) = run_sql(cassie, &sql(metric.name())) {
        eprintln!(
            "embedding_workloads: {} index with metric {} unsupported ({error}); building l2 so queries show the labelled fallback",
            plan.algorithm,
            metric.name()
        );
        run_sql(cassie, &sql("l2"))?;
        let _ = write!(
            options,
            " [metric {} unsupported by this index; l2 index built]",
            metric.name()
        );
    }
    let build_ms = started.elapsed().as_secs_f64() * 1e3;
    let artifact_bytes = cassie
        .midge
        .vector_index_artifact_footprint_for_diagnostics(TABLE, "embedding")
        .map_or(0, |footprint| {
            footprint["total_bytes"].as_u64().unwrap_or(0)
        });
    Ok(IndexStats {
        options,
        build_ms,
        artifact_bytes,
    })
}

type TruthMap = std::collections::BTreeMap<String, Vec<Truth>>;
type QuerySets = [(String, Vec<usize>)];

fn measure_cells(
    measurement: &Measurement<'_>,
    truths: &TruthMap,
    query_sets: &QuerySets,
) -> Result<Vec<Cell>, String> {
    let workload = measurement.workload;
    // Warmup is separate from measurement and is not reported.
    let (warm_split, warm_all) = &query_sets[0];
    let warm: Vec<usize> = if measurement.recall_only {
        Vec::new()
    } else {
        warm_all.iter().copied().take(WARMUP_QUERIES).collect()
    };
    for (filter, _) in FILTERS.iter().filter(|_| !warm.is_empty()) {
        let column = (*filter != "unfiltered").then_some(*filter);
        let warm_truth: Vec<Truth> = truths[&format!("{filter}:{warm_split}")]
            .iter()
            .take(warm.len())
            .cloned()
            .collect();
        measurement.run(&warm, &warm_truth, column)?;
    }
    let mut cells = Vec::new();
    for (filter, selectivity) in FILTERS {
        let column = (filter != "unfiltered").then_some(filter);
        let subset_size = selectivity.map_or(workload.document_ids.len(), |s| {
            workload.tenant_subset(s).len()
        });
        for (split, queries) in query_sets {
            let outcomes =
                measurement.run(queries, &truths[&format!("{filter}:{split}")], column)?;
            let plan = explain(
                measurement.cassie,
                &query_sql(workload, queries[0], measurement.metric, column),
            );
            cells.push(Cell {
                filter,
                selectivity,
                subset_size,
                split: split.clone(),
                plan,
                outcomes,
            });
        }
    }
    Ok(cells)
}

fn run_configuration(
    workload: &Workload,
    options: &RunOptions,
    base: &Path,
    metric: Metric,
    plan: Option<&IndexPlan>,
    truths: &TruthMap,
    query_sets: &QuerySets,
) -> Result<Vec<Json>, String> {
    let name = plan.map_or_else(
        || "exact".to_string(),
        |p| format!("{}-{}", p.algorithm, p.point),
    );
    let work = options.work_dir.join(format!(
        "{}-{}-{}-{name}",
        workload.model_key,
        workload.set,
        metric.name()
    ));
    let _ = std::fs::remove_dir_all(&work);
    copy_dir(base, &work)?;
    let before_bytes = dir_bytes(&work);
    let cassie = open(
        &work,
        workload.dimensions,
        options.query_memory_budget_bytes,
    )?;
    let stats = build_index(&cassie, plan, metric)?;
    let growth = dir_bytes(&work).saturating_sub(before_bytes);
    let measurement = Measurement {
        cassie: &cassie,
        workload,
        metric,
        indexed: plan.is_some(),
        recall_only: options.recall_only,
    };
    let cells = measure_cells(&measurement, truths, query_sets)?;
    let context = RowContext {
        workload,
        metric,
        algorithm: plan.map_or("exact", |p| p.algorithm),
        point: plan.map_or("none", |p| p.point.as_str()),
        options: &stats.options,
        build_ms: stats.build_ms,
        index_artifact_bytes: stats.artifact_bytes,
        index_dir_growth_bytes: growth,
        peak_memory_bytes: counter(&cassie.metrics(), "query", "peak_accounted_memory_bytes"),
    };
    let mut rows = Vec::new();
    for cell in &cells {
        let all: Vec<&QueryOutcome> = cell.outcomes.iter().collect();
        let groups: Vec<(&str, Vec<&QueryOutcome>)> = if plan.is_some() {
            vec![
                (
                    "ann",
                    all.iter().copied().filter(|o| o.served_by_ann).collect(),
                ),
                (
                    "exact_fallback",
                    all.iter().copied().filter(|o| !o.served_by_ann).collect(),
                ),
            ]
        } else {
            vec![("exact", all)]
        };
        for (kind, group) in groups.iter().filter(|(_, group)| !group.is_empty()) {
            rows.push(summarize(&context, cell, kind, group));
        }
    }
    cassie.shutdown();
    drop(cassie);
    let _ = std::fs::remove_dir_all(&work);
    Ok(rows)
}

/// Runs every configuration for one verified workload and returns the result document.
///
/// # Errors
///
/// Returns a message when ground truth, ingest or a configuration fails.
pub fn run_workload(
    manifest: &Manifest,
    workload: &Workload,
    options: &RunOptions,
) -> Result<Json, String> {
    std::fs::create_dir_all(&options.work_dir).map_err(|error| error.to_string())?;
    let k = manifest.ground_truth.k;
    let query_sets: Vec<(String, Vec<usize>)> = SPLITS
        .iter()
        .filter(|split| options.splits.iter().any(|chosen| chosen == *split))
        .map(|split| {
            workload
                .split_queries(split, options.query_limit, options.query_range.as_ref())
                .map(|queries| ((*split).to_string(), queries))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let (base, ingest_seconds) = build_base(workload, options)?;
    let mut rows = Vec::new();
    let mut truth_seconds = 0.0;
    for metric_name in manifest
        .ground_truth
        .metrics
        .iter()
        .filter(|name| options.metrics.is_empty() || options.metrics.contains(name))
    {
        let metric = if metric_name == "cosine" {
            Metric::Cosine
        } else {
            Metric::L2
        };
        let started = Instant::now();
        let mut truths = TruthMap::new();
        for (filter, selectivity) in FILTERS {
            let subset = selectivity.map(|s| workload.tenant_subset(s));
            for (split, queries) in &query_sets {
                truths.insert(
                    format!("{filter}:{split}"),
                    workload.truths(
                        metric,
                        &format!("{filter}-{split}"),
                        subset.as_deref(),
                        queries,
                        k,
                    )?,
                );
            }
        }
        truth_seconds += started.elapsed().as_secs_f64();
        let plans = index_plans(
            workload.document_ids.len(),
            options.tuning_points,
            &manifest.index_tuning,
            options.sweep,
        );
        if !options.recall_only
            && match options.product_configuration {
                Some(Configuration::Exact) => true,
                Some(Configuration::Hnsw(_) | Configuration::Ivfflat(_)) => false,
                None => options.algorithms.iter().any(|name| name == "exact"),
            }
        {
            rows.extend(run_configuration(
                workload,
                options,
                &base,
                metric,
                None,
                &truths,
                &query_sets,
            )?);
        }
        let selected_plans: Vec<IndexPlan> = match options.product_configuration {
            Some(Configuration::Exact) => Vec::new(),
            Some(Configuration::Hnsw(point)) => plans
                .iter()
                .filter(|plan| plan.algorithm == "hnsw")
                .nth(point)
                .cloned()
                .into_iter()
                .collect(),
            Some(Configuration::Ivfflat(point)) => plans
                .iter()
                .filter(|plan| plan.algorithm == "ivfflat")
                .nth(point)
                .cloned()
                .into_iter()
                .collect(),
            None => plans
                .into_iter()
                .filter(|plan| options.algorithms.iter().any(|name| name == plan.algorithm))
                .collect(),
        };
        if matches!(
            options.product_configuration,
            Some(Configuration::Hnsw(_) | Configuration::Ivfflat(_))
        ) && selected_plans.is_empty()
        {
            return Err(
                "selected product vector tuning point is absent from the manifest".to_string(),
            );
        }
        for plan in &selected_plans {
            rows.extend(run_configuration(
                workload,
                options,
                &base,
                metric,
                Some(plan),
                &truths,
                &query_sets,
            )?);
        }
    }
    let _ = std::fs::remove_dir_all(&base);
    Ok(json!({
        "issue": 1365,
        "model": workload.model_key,
        "model_id": workload.model_id,
        "model_revision": workload.model_revision,
        "set": workload.set,
        "documents_sha256": workload.documents_sha256,
        "queries_sha256": workload.queries_sha256,
        "ingest_seconds": ingest_seconds,
        "ground_truth_seconds": truth_seconds,
        "query_memory_budget_bytes": options.query_memory_budget_bytes,
        "query_timeout_ms": 0,
        "tuning_points_per_index": options.tuning_points,
        "sweep": options.sweep,
        "recall_only": options.recall_only,
        "splits": options.splits,
        "query_limit": options.query_limit,
        "query_range": options.query_range.as_ref().map(|range| format!("{}..{}", range.start, range.end)),
        "product_configuration": options.product_configuration.map(|configuration| match configuration {
            Configuration::Exact => "exact",
            Configuration::Hnsw(0) => "hnsw-1",
            Configuration::Hnsw(_) => "hnsw-2",
            Configuration::Ivfflat(0) => "ivfflat-1",
            Configuration::Ivfflat(_) => "ivfflat-2",
        }),
        "note": "peak_query_memory_bytes_configuration_scope is the process-lifetime peak of one configuration, shared by its filter rows.",
        "rows": rows,
    }))
}
