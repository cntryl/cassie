//! Opt-in real-embedding recall/latency workloads for #1365.
//!
//! Gate: `CASSIE_BENCH_EMBEDDING_WORKLOADS=1`. Without it this target prints one line and
//! exits, so `cargo bench` stays cheap. Generate the vectors first with
//! `tools/embeddings/README.md`. Optional settings:
//!
//! - `CASSIE_BENCH_EMBEDDING_SETS` (default `10k`), comma list of manifest set names
//! - `CASSIE_BENCH_EMBEDDING_MODELS` (default `bge-small,bge-base,synthetic`; `synthetic` is the
//!   deterministic control generated in memory from the manifest `synthetic_control`)
//! - `CASSIE_BENCH_EMBEDDING_TUNING_POINTS` (default `2`, maximum `2`) per index
//! - `CASSIE_BENCH_EMBEDDING_QUERY_LIMIT` caps queries per split (smoke runs only)
//! - `CASSIE_BENCH_EMBEDDING_METRICS` (default `l2,cosine`)
//! - `CASSIE_BENCH_EMBEDDING_ALGORITHMS` (default `exact,hnsw,ivfflat`)
//! - `CASSIE_BENCH_EMBEDDING_SWEEP=1` builds every swept candidate in the manifest
//!   `index_tuning` grid and measures the tuning split only; selection uses these rows
//! - `CASSIE_BENCH_EMBEDDING_RECALL_ONLY=1` skips the exact configuration and warmup and
//!   reports recall and candidate counts (latency columns are not-quiet and indicative)
//! - `CASSIE_BENCH_EMBEDDINGS_DIR` reads and writes this directory instead of
//!   `target/embeddings`
//! - `CASSIE_BENCH_EMBEDDING_QUERY_MEMORY_BUDGET_BYTES` (default 8 GiB; Cassie's 10 MiB
//!   default rejects exact scans of these sets)
//!
//! Raw results go to `target/embeddings/results/*.json`.
//!
//! The product runner uses `CASSIE_BENCH_PRODUCT_PROFILE=smoke|full` and
//! `CASSIE_BENCH_PRODUCT_OUTPUT_DIR`. Full mode also requires the exact
//! `CASSIE_BENCH_PRODUCT_VECTOR_CONFIGURATION` selector (`exact`, `hnsw-1`,
//! `hnsw-2`, `ivfflat-1`, or `ivfflat-2`) and one `CASSIE_BENCH_PRODUCT_VECTOR_QUERY_RANGE`
//! shard (`0..250`, `250..500`, `500..750`, or `750..1000`). Each range is used
//! independently for both the tuning and held-out query splits.

#[path = "support/embedding_workloads.rs"]
mod embedding_workloads;
#[path = "support/embedding_workloads/loader.rs"]
mod loader;
#[path = "support/embedding_workloads/product_scratch.rs"]
mod product_scratch;
#[path = "support/embedding_workloads/runner.rs"]
mod runner;

use std::path::PathBuf;

use embedding_workloads::manifest::{embeddings_dir, Manifest};
use embedding_workloads::product_selection::{self, Configuration};
use loader::Workload;

const ENV_FLAG: &str = "CASSIE_BENCH_EMBEDDING_WORKLOADS";

fn env_list(name: &str, default: &str) -> Vec<String> {
    std::env::var(name)
        .unwrap_or_else(|_| default.to_string())
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

fn env_number(name: &str) -> Option<usize> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
}

fn print_summary(result: &serde_json::Value, split: &str) {
    let rows = result["rows"].as_array().map_or(&[][..], Vec::as_slice);
    for row in rows.iter().filter(|row| row["split"] == split) {
        println!(
            "embedding_workloads model={} set={} metric={} algorithm={} point={} filter={} kind={} \
             queries={}/{} recall20={:.4} p50_us={:.0} p95_us={:.0} qps={:.1} candidates={:.0} \
             build_ms={:.0} index_bytes={} fallback_rate={:.3}",
            row["model"].as_str().unwrap_or(""),
            row["set"].as_str().unwrap_or(""),
            row["metric"].as_str().unwrap_or(""),
            row["algorithm"].as_str().unwrap_or(""),
            row["tuning_point"].as_str().unwrap_or(""),
            row["filter"].as_str().unwrap_or(""),
            row["row_kind"].as_str().unwrap_or(""),
            row["queries"],
            row["queries_in_group"],
            row["recall_at_20_mean"].as_f64().unwrap_or(0.0),
            row["latency_p50_us"].as_f64().unwrap_or(0.0),
            row["latency_p95_us"].as_f64().unwrap_or(0.0),
            row["qps"].as_f64().unwrap_or(0.0),
            row["candidates_mean"].as_f64().unwrap_or(0.0),
            row["build_ms"].as_f64().unwrap_or(0.0),
            row["index_artifact_bytes"],
            row["fallback_rate"].as_f64().unwrap_or(0.0),
        );
    }
}

fn run() -> Result<(), String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest = Manifest::load(&root.join("tests/corpus/embeddings/manifest.json"))?;
    manifest.validate()?;
    let dir = std::env::var_os("CASSIE_BENCH_EMBEDDINGS_DIR")
        .map_or_else(|| embeddings_dir(&root), PathBuf::from);
    let sweep = std::env::var_os("CASSIE_BENCH_EMBEDDING_SWEEP").is_some();
    let results_dir = dir.join("results");
    std::fs::create_dir_all(&results_dir).map_err(|error| error.to_string())?;
    let options = runner::RunOptions {
        tuning_points: env_number("CASSIE_BENCH_EMBEDDING_TUNING_POINTS").unwrap_or(2),
        splits: if sweep {
            vec!["tuning".to_string()]
        } else {
            vec!["tuning".to_string(), "held_out".to_string()]
        },
        sweep,
        metrics: env_list("CASSIE_BENCH_EMBEDDING_METRICS", ""),
        algorithms: env_list("CASSIE_BENCH_EMBEDDING_ALGORITHMS", "exact,hnsw,ivfflat"),
        recall_only: std::env::var_os("CASSIE_BENCH_EMBEDDING_RECALL_ONLY").is_some(),
        query_limit: env_number("CASSIE_BENCH_EMBEDDING_QUERY_LIMIT"),
        query_range: None,
        product_configuration: None,
        work_dir: dir.join("work"),
        query_memory_budget_bytes: env_number("CASSIE_BENCH_EMBEDDING_QUERY_MEMORY_BUDGET_BYTES")
            .unwrap_or(8 << 30),
    };
    for set in env_list("CASSIE_BENCH_EMBEDDING_SETS", "10k") {
        for model in env_list(
            "CASSIE_BENCH_EMBEDDING_MODELS",
            "bge-small,bge-base,synthetic",
        ) {
            let workload = if model == manifest.synthetic_control.name {
                loader::Workload::synthetic(&manifest, &dir, &set)?
            } else {
                loader::Workload::load(&manifest, &dir, &model, &set)?
            };
            let result = runner::run_workload(&manifest, &workload, &options)?;
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_secs());
            let mode = if sweep { "-sweep" } else { "" };
            let path = results_dir.join(format!("{stamp}-{model}-{set}{mode}.json"));
            std::fs::write(
                &path,
                serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?,
            )
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
            print_summary(&result, if sweep { "tuning" } else { "held_out" });
            println!("embedding_workloads wrote {}", path.display());
        }
    }
    Ok(())
}

fn product_smoke_workload(manifest: &Manifest, dir: &std::path::Path) -> Workload {
    let control = &manifest.synthetic_control;
    let vectors = embedding_workloads::synthetic::generate(control, 128, 8);
    Workload {
        model_key: "synthetic-smoke".to_string(),
        model_stem: "synthetic-smoke".to_string(),
        model_id: "deterministic-small-synthetic".to_string(),
        model_revision: "manifest synthetic_control streams".to_string(),
        set: "synthetic-smoke".to_string(),
        dimensions: control.dimensions,
        documents_sha256: embedding_workloads::synthetic::checksum(&vectors.documents),
        queries_sha256: embedding_workloads::synthetic::checksum(&vectors.queries),
        documents: vectors.documents,
        document_ids: (0..128).map(|id| format!("smoke-doc-{id:03}")).collect(),
        queries: vectors.queries,
        query_ids: (0..8).map(|id| format!("smoke-query-{id:02}")).collect(),
        tuning_count: 4,
        dir: dir.to_path_buf(),
    }
}

fn write_product_shard(
    output_dir: &std::path::Path,
    configuration: &str,
    range: Option<&std::ops::Range<usize>>,
    report: &mut serde_json::Value,
) -> Result<(), String> {
    let relative = range.map_or_else(
        || "smoke.json".to_string(),
        |range| format!("{}-{}.json", range.start, range.end),
    );
    let path = output_dir
        .join("vector-shards")
        .join(configuration)
        .join(relative);
    let parent = path.parent().expect("product shard parent");
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    report["product_metric"] = serde_json::json!("vector_cosine_top20");
    report["product_profile"] = serde_json::json!(if range.is_some() { "full" } else { "smoke" });
    report["configuration"] = serde_json::json!(configuration);
    report["query_range"] = range
        .map(|value| format!("{}..{}", value.start, value.end))
        .into();
    let temporary = path.with_extension("json.tmp");
    std::fs::write(
        &temporary,
        serde_json::to_vec_pretty(report).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("cannot write {}: {error}", temporary.display()))?;
    std::fs::rename(&temporary, &path)
        .map_err(|error| format!("cannot publish {}: {error}", path.display()))?;
    println!("product_vector_shard={}", path.display());
    Ok(())
}

fn run_product(profile: &str) -> Result<(), String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest = Manifest::load(&root.join("tests/corpus/embeddings/manifest.json"))?;
    manifest.validate()?;
    let output_dir = std::env::var_os("CASSIE_BENCH_PRODUCT_OUTPUT_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| "CASSIE_BENCH_PRODUCT_OUTPUT_DIR is required".to_string())?;
    let temporary_root =
        std::env::var_os("RUNNER_TEMP").map_or_else(std::env::temp_dir, PathBuf::from);
    let artifact_output = std::env::var_os("STRESS_OUTPUT_DIR").map(PathBuf::from);
    let scratch = product_scratch::ProductScratch::create(
        &temporary_root,
        &output_dir,
        artifact_output.as_deref(),
    )
    .map_err(|error| format!("cannot create vector product scratch: {error}"))?;
    let truth_cache_dir = scratch.truth_cache_dir();
    std::fs::create_dir(&truth_cache_dir).map_err(|error| error.to_string())?;
    let (configuration, query_range, workload) = match profile {
        "smoke" => (
            Configuration::Exact,
            None,
            product_smoke_workload(&manifest, &truth_cache_dir),
        ),
        "full" => {
            let config = product_selection::configuration(
                &std::env::var("CASSIE_BENCH_PRODUCT_VECTOR_CONFIGURATION").map_err(|_| {
                    "CASSIE_BENCH_PRODUCT_VECTOR_CONFIGURATION is required for full profile"
                        .to_string()
                })?,
            )?;
            let range = product_selection::query_range(
                &std::env::var("CASSIE_BENCH_PRODUCT_VECTOR_QUERY_RANGE").map_err(|_| {
                    "CASSIE_BENCH_PRODUCT_VECTOR_QUERY_RANGE is required for full profile"
                        .to_string()
                })?,
            )?;
            product_selection::validate_split_range(&range, manifest.queries.tuning.count)?;
            product_selection::validate_split_range(&range, manifest.queries.held_out.count)?;
            let embeddings_dir = std::env::var_os("CASSIE_BENCH_EMBEDDINGS_DIR")
                .map_or_else(|| embeddings_dir(&root), PathBuf::from);
            let mut workload = Workload::load(&manifest, &embeddings_dir, "bge-base", "100k")?;
            if workload.dimensions != 768 || workload.document_ids.len() != 100_000 {
                return Err("pinned BGE-base product fixture must be 100k x 768".to_string());
            }
            workload.dir = truth_cache_dir;
            (config, Some(range), workload)
        }
        _ => return Err(format!("unsupported product profile {profile:?}")),
    };
    let selected_algorithms = match configuration {
        Configuration::Exact => vec!["exact".to_string()],
        Configuration::Hnsw(_) => vec!["hnsw".to_string()],
        Configuration::Ivfflat(_) => vec!["ivfflat".to_string()],
    };
    let options = runner::RunOptions {
        tuning_points: 2,
        splits: vec!["tuning".to_string(), "held_out".to_string()],
        sweep: false,
        metrics: vec!["cosine".to_string()],
        algorithms: selected_algorithms,
        recall_only: false,
        query_limit: None,
        query_range: query_range.clone(),
        product_configuration: Some(configuration),
        work_dir: scratch.work_dir(),
        query_memory_budget_bytes: 8 << 30,
    };
    let mut report = runner::run_workload(&manifest, &workload, &options)?;
    let selector = match configuration {
        Configuration::Exact => "exact",
        Configuration::Hnsw(0) => "hnsw-1",
        Configuration::Hnsw(_) => "hnsw-2",
        Configuration::Ivfflat(0) => "ivfflat-1",
        Configuration::Ivfflat(_) => "ivfflat-2",
    };
    write_product_shard(&output_dir, selector, query_range.as_ref(), &mut report)
}

fn main() {
    if let Ok(profile) = std::env::var("CASSIE_BENCH_PRODUCT_PROFILE") {
        if let Err(message) = run_product(&profile) {
            eprintln!("embedding_product_workloads failed: {message}");
            std::process::exit(1);
        }
        return;
    }
    if std::env::var_os(ENV_FLAG).is_none() {
        println!("embedding_workloads skipped: set {ENV_FLAG}=1 (see tools/embeddings/README.md)");
        return;
    }
    if let Err(message) = run() {
        eprintln!("embedding_workloads failed: {message}");
        std::process::exit(1);
    }
}
