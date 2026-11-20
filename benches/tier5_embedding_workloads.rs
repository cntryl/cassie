//! Opt-in real-embedding recall/latency workloads for #1365 (workstation only, not CI).
//!
//! Gate: `CASSIE_BENCH_EMBEDDING_WORKLOADS=1`. Without it this target prints one line and
//! exits, so `cargo bench` stays cheap. Generate the vectors first with
//! `tools/embeddings/README.md`. Optional settings:
//!
//! - `CASSIE_BENCH_EMBEDDING_SETS` (default `10k`), comma list of manifest set names
//! - `CASSIE_BENCH_EMBEDDING_MODELS` (default `bge-small,bge-base`)
//! - `CASSIE_BENCH_EMBEDDING_TUNING_POINTS` (default `2`, maximum `2`) per index
//! - `CASSIE_BENCH_EMBEDDING_QUERY_LIMIT` caps queries per split (smoke runs only)
//! - `CASSIE_BENCH_EMBEDDING_QUERY_MEMORY_BUDGET_BYTES` (default 8 GiB; Cassie's 10 MiB
//!   default rejects exact scans of these sets)
//!
//! Raw results go to `target/embeddings/results/*.json`.

#[path = "support/embedding_workloads.rs"]
mod embedding_workloads;
#[path = "support/embedding_workloads/loader.rs"]
mod loader;
#[path = "support/embedding_workloads/runner.rs"]
mod runner;

use std::path::PathBuf;

use embedding_workloads::manifest::{embeddings_dir, Manifest};

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

fn print_summary(result: &serde_json::Value) {
    let rows = result["rows"].as_array().map_or(&[][..], Vec::as_slice);
    for row in rows.iter().filter(|row| row["split"] == "held_out") {
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
    let dir = embeddings_dir(&root);
    let results_dir = dir.join("results");
    std::fs::create_dir_all(&results_dir).map_err(|error| error.to_string())?;
    let options = runner::RunOptions {
        tuning_points: env_number("CASSIE_BENCH_EMBEDDING_TUNING_POINTS").unwrap_or(2),
        query_limit: env_number("CASSIE_BENCH_EMBEDDING_QUERY_LIMIT"),
        work_dir: dir.join("work"),
        query_memory_budget_bytes: env_number("CASSIE_BENCH_EMBEDDING_QUERY_MEMORY_BUDGET_BYTES")
            .unwrap_or(8 << 30),
    };
    for set in env_list("CASSIE_BENCH_EMBEDDING_SETS", "10k") {
        for model in env_list("CASSIE_BENCH_EMBEDDING_MODELS", "bge-small,bge-base") {
            let workload = loader::Workload::load(&manifest, &dir, &model, &set)?;
            let result = runner::run_workload(&manifest, &workload, &options)?;
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_secs());
            let path = results_dir.join(format!("{stamp}-{model}-{set}.json"));
            std::fs::write(
                &path,
                serde_json::to_string_pretty(&result).map_err(|e| e.to_string())?,
            )
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
            print_summary(&result);
            println!("embedding_workloads wrote {}", path.display());
        }
    }
    Ok(())
}

fn main() {
    if std::env::var_os(ENV_FLAG).is_none() {
        println!("embedding_workloads skipped: set {ENV_FLAG}=1 (see tools/embeddings/README.md)");
        return;
    }
    if let Err(message) = run() {
        eprintln!("embedding_workloads failed: {message}");
        std::process::exit(1);
    }
}
