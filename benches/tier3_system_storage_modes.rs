//! Tier 3 representative row, column and mixed SQL workloads for #1113.
//! Uses the registered fixed-duration harness at exactly 100k fact rows.
//! Storage-mode result equality is checked before timing; no latency threshold
//! is asserted by these correctness guards.

use std::sync::Arc;
use std::time::Instant;

use cassie::app::{Cassie, CassieSession};
use cassie::config::CassieRuntimeConfig;
use cassie::types::Value;

#[path = "tier3_system_storage_modes/data.rs"]
mod data;
#[path = "tier3_system_storage_modes/expected.rs"]
mod expected;
#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "tier3_system_storage_modes/setup.rs"]
mod setup;
#[path = "support/stress.rs"]
pub mod stress;

const BENCHMARK: &str = "tier3_system_storage_modes";
const FACT_ROWS: usize = 100_000;
const QUERY_BUDGET: usize = 2 * 1024 * 1024 * 1024;

struct Db {
    cassie: Arc<Cassie>,
    session: CassieSession,
}

impl Db {
    fn new(profile: setup::NativeProfile) -> Self {
        let mut config = CassieRuntimeConfig::from_env().expect("benchmark config");
        config.limits.max_result_rows = FACT_ROWS * 2;
        config.limits.query_memory_budget_bytes = QUERY_BUDGET;
        config.limits.query_timeout_ms = 0;
        profile.configure(&mut config);
        let cassie = Arc::new(Cassie::new_in_memory_with_config(config).expect("memory Cassie"));
        cassie.startup().expect("startup");
        let session = cassie.create_session("benchmark", None);
        let db = Self { cassie, session };
        println!("native_memory_startup={}", setup::native_state(&db));
        db
    }

    fn exec(&self, sql: &str) -> Vec<Vec<Value>> {
        self.cassie
            .execute_sql(&self.session, sql, vec![])
            .unwrap_or_else(|error| {
                eprintln!("storage_sql_error={}", serde_json::json!({"sql": sql, "error": error.to_string(), "native": setup::native_state(self)}));
                panic!("benchmark SQL failed: {sql}: {error}")
            })
            .rows()
            .to_vec()
    }
}

fn rendered(rows: &[Vec<Value>]) -> Vec<String> {
    let mut result: Vec<_> = rows.iter().map(|row| format!("{row:?}")).collect();
    result.sort();
    result
}

fn cases() -> Vec<(&'static str, String, String)> {
    let mut result = Vec::new();
    for (mode, fact) in [("row", "f_row"), ("column", "f_col")] {
        for (shape, query) in [
            ("full_scan", data::full_scan_sql as fn(&str) -> String),
            ("narrow_projection", data::narrow_sql),
            ("filtered_aggregate", data::filtered_agg_sql),
        ] {
            let name = match (mode, shape) {
                ("row", "full_scan") => "row_full_scan",
                ("column", "full_scan") => "column_full_scan",
                ("row", "narrow_projection") => "row_narrow_projection",
                ("column", "narrow_projection") => "column_narrow_projection",
                ("row", _) => "row_filtered_aggregate",
                _ => "column_filtered_aggregate",
            };
            result.push((name, query(fact), query("f_row")));
        }
    }
    for (name, fact, dim) in [
        ("row_join_row", "f_row", "d_row"),
        ("column_join_column", "f_col", "d_col"),
        ("row_join_column", "f_row", "d_col"),
        ("column_join_row", "f_col", "d_row"),
    ] {
        result.push((
            name,
            data::join_agg_sql(fact, dim),
            data::join_agg_sql("f_row", "d_row"),
        ));
    }
    result
}

fn preflight(db: &Db, sql: &str) -> (String, String) {
    let rows = db.exec(&format!("EXPLAIN {sql}"));
    let Some(Value::String(plan)) = rows.first().and_then(|row| row.first()) else {
        panic!("EXPLAIN must return a textual plan");
    };
    let read = |name: &str| {
        plan.split_ascii_whitespace().find_map(|token| {
            let (key, value) = token.split_once('=')?;
            (key == name).then(|| {
                value
                    .trim_matches(|c: char| matches!(c, ',' | '|' | '[' | ']'))
                    .to_owned()
            })
        })
    };
    (
        read("access_path").expect("observed access path"),
        read("fallback_reason").expect("observed fallback reason"),
    )
}

fn selected_case(runner: &stress::CassieStressRunner, name: &str) -> Option<stress::StressCase> {
    let case = stress::StressCase::new(name, "100k")
        .runtime_contract(
            stress::FixtureDeclaration::new(
                performance_benchmarks::FixtureClass::Representative,
                FACT_ROWS,
                "tier3_system_storage_modes/100k",
            ),
            stress::OperationUnit::Query,
        )
        .metadata("query_memory_budget_bytes", QUERY_BUDGET.to_string())
        .metadata("benchmark_resource_profile", "storage_modes_100k_2g")
        .metadata("storage_mode", "memory")
        .metadata(
            "native_memory_budget_bytes",
            setup::NATIVE_BUDGET.to_string(),
        )
        .metadata(
            "native_memtable_size_bytes",
            setup::MEMTABLE_LIMIT.to_string(),
        )
        .metadata(
            "fact_authority",
            if name.starts_with("column") {
                "column"
            } else {
                "row"
            },
        )
        .metadata(
            "dimension_authority",
            if name.ends_with("_column") {
                "column"
            } else if name.contains("_join_") {
                "row"
            } else {
                "none"
            },
        )
        .metadata("dimension_rows", data::DIM_ROWS.to_string());
    runner.is_enabled(&case).then_some(case)
}

type SelectedCase = (&'static str, stress::StressCase, String, String);

fn default_native_probe() -> std::process::ExitCode {
    println!(
        "native_probe={} ",
        serde_json::json!({"mode": "default-native-probe", "qualification": false, "rows": FACT_ROWS, "query_memory_budget_bytes": QUERY_BUDGET, "native_settings": null})
    );
    let db = Db::new(setup::NativeProfile::DefaultProbe);
    match db.seed() {
        Ok(()) => {
            println!("native_probe_setup_complete=true");
            std::process::ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("native_probe_setup_failed={error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run_queries(
    runner: &mut stress::CassieStressRunner,
    db: &Db,
    setup_time: &str,
    selected: Vec<SelectedCase>,
) {
    let mut validation_receipts = Vec::new();
    for (name, case, sql, baseline) in selected {
        let independent = expected::rows(name, FACT_ROWS);
        let expected_fingerprint = expected::fingerprint(&independent);
        let expected = rendered(&db.exec(&baseline));
        assert_eq!(
            expected,
            rendered(&independent),
            "independent relational oracle"
        );
        drop(independent);
        assert!(!expected.is_empty(), "baseline must have rows");
        assert_eq!(rendered(&db.exec(&sql)), expected, "storage-mode equality");
        let (access, fallback) = preflight(db, &sql);
        let native_before = setup::native_state(db);
        println!(
            "native_memory_interval={}",
            serde_json::json!({"phase": "before", "workload": name, "native": native_before})
        );
        let validation_before = db.cassie.midge.column_validation_reads_for_diagnostics();
        let configuration = db.cassie.midge.native_memory_configuration();
        let case = case
            .metadata(
                "column_validation_reads_before_measure_batch",
                validation_before.to_string(),
            )
            .metadata(
                "native_configured_total_bytes",
                configuration.total.to_string(),
            )
            .metadata(
                "native_configured_memtable_bytes",
                configuration.memtable_limit.to_string(),
            )
            .metadata(
                "native_configured_flush_threshold_bytes",
                configuration.flush_threshold.to_string(),
            )
            .metadata(
                "native_configured_transaction_pool_bytes",
                configuration.transaction_pool.to_string(),
            )
            .metadata("setup_time_ns", setup_time)
            .metadata("result_cardinality", expected.len().to_string())
            .preflight_evidence(access, fallback)
            .runtime_evidence(Arc::clone(&db.cassie));
        runner.measure_batch(case, 1, || {
            let rows = db.exec(&sql);
            assert_eq!(rows.len(), expected.len(), "timed result cardinality");
            assert_eq!(
                expected::fingerprint(&rows),
                expected_fingerprint,
                "timed result contents"
            );
            std::hint::black_box(rows.len())
        });
        let validation_after = db.cassie.midge.column_validation_reads_for_diagnostics();
        let native_after = setup::native_state(db);
        let receipt = serde_json::json!({"workload": name, "validation_reads_before": validation_before, "validation_reads_after": validation_after, "validation_reads_delta": validation_after.saturating_sub(validation_before), "interval": "measure_batch_including_harness_warmup", "native_before": native_before, "native_after": native_after});
        println!("column_validation_reads={receipt}");
        validation_receipts.push(receipt);
    }
    if let Ok(path) = std::env::var("BENCH_TIER3_STORAGE_MODES_VALIDATION_READS_OUT") {
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&validation_receipts).expect("validation receipts"),
        )
        .expect("write validation read receipts");
    }
}

fn main() -> std::process::ExitCode {
    std::env::set_var("CASSIE_STORAGE_MODE", "memory");
    std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
    let mode = std::env::var("BENCH_TIER3_STORAGE_MODES_MODE")
        .unwrap_or_else(|_| "qualification".to_owned());
    if mode == "default-native-probe" {
        return default_native_probe();
    }
    assert_eq!(
        mode, "qualification",
        "unsupported storage-mode benchmark mode"
    );
    let mut runner = stress::runner(performance_benchmarks::BenchmarkTier::Tier3, BENCHMARK);
    let selected: Vec<_> = cases()
        .into_iter()
        .filter_map(|(name, sql, baseline)| {
            selected_case(&runner, name).map(|case| (name, case, sql, baseline))
        })
        .collect();
    if selected.is_empty() {
        runner.finish();
        return std::process::ExitCode::SUCCESS;
    }
    let setup_started = Instant::now();
    let db = Db::new(setup::NativeProfile::Qualification);
    if let Err(error) = db.seed() {
        eprintln!("qualification_setup_failed={error}");
        return std::process::ExitCode::FAILURE;
    }
    let setup_time = setup_started.elapsed().as_nanos().to_string();
    run_queries(&mut runner, &db, &setup_time, selected);
    runner.finish();
    std::process::ExitCode::SUCCESS
}
