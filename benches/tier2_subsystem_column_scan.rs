#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
#[path = "support/workloads.rs"]
mod workloads;

const ROWS: usize = 2_048;
const QUERIES_PER_SAMPLE: usize = 8;
const FIXTURE_ID: &str = "tier2_subsystem_column_scan/2k";
const COMPRESSIBLE_CANDIDATE: &str = "perf.column.selective_encoded_scan.2k";
const COMPRESSIBLE_BASELINE: &str = "perf.column.selective_plain_scan_baseline.2k";
const INCOMPRESSIBLE_CANDIDATE: &str = "perf.column.incompressible_adaptive_scan.2k";
const INCOMPRESSIBLE_BASELINE: &str = "perf.column.incompressible_plain_scan_baseline.2k";

fn main() {
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier2,
        "tier2_subsystem_column_scan",
    );
    let compressible = case("selective_encoded_scan");
    let compressible_baseline = case("selective_plain_scan_baseline");
    let incompressible = case("incompressible_adaptive_scan");
    let incompressible_baseline = case("incompressible_plain_scan_baseline");
    let selections = [
        runner.is_enabled(&compressible),
        runner.is_enabled(&compressible_baseline),
        runner.is_enabled(&incompressible),
        runner.is_enabled(&incompressible_baseline),
    ];
    if selections.iter().any(|selected| *selected) {
        let setup_started = std::time::Instant::now();
        let runtime = workloads::runtime();
        let context = runtime
            .block_on(workloads::column_codec_acceptance_context(ROWS))
            .expect("prepare Tier 2 column codec acceptance fixture");
        let setup_time = setup_started.elapsed().as_nanos().max(1).to_string();

        measure_selected(
            &mut runner,
            &context,
            compressible,
            selections[0],
            workloads::COMPRESSIBLE_AUTO_SQL,
            &setup_time,
        );
        measure_selected(
            &mut runner,
            &context,
            compressible_baseline,
            selections[1],
            workloads::COMPRESSIBLE_PLAIN_SQL,
            &setup_time,
        );
        measure_selected(
            &mut runner,
            &context,
            incompressible,
            selections[2],
            workloads::INCOMPRESSIBLE_AUTO_SQL,
            &setup_time,
        );
        measure_selected(
            &mut runner,
            &context,
            incompressible_baseline,
            selections[3],
            workloads::INCOMPRESSIBLE_PLAIN_SQL,
            &setup_time,
        );
        if selections[0] && selections[1] {
            runner.require_relative_p95(COMPRESSIBLE_CANDIDATE, COMPRESSIBLE_BASELINE, 0.85);
        }
        if selections[2] && selections[3] {
            runner.require_relative_p95(INCOMPRESSIBLE_CANDIDATE, INCOMPRESSIBLE_BASELINE, 1.05);
        }
    }
    runner.finish();
}

fn case(workload: &str) -> stress::StressCase {
    stress::StressCase::new(workload, "2k").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Subsystem,
            ROWS,
            FIXTURE_ID,
        ),
        stress::OperationUnit::ResultRow,
    )
}

fn measure_selected(
    runner: &mut stress::CassieStressRunner,
    context: &workloads::BenchContext,
    case: stress::StressCase,
    selected: bool,
    sql: &str,
    setup_time: &str,
) {
    if !selected {
        return;
    }
    let before = context.cassie.metrics();
    runner.measure_counted(case.metadata("setup_time_ns", setup_time), || {
        let mut completed_rows = 0usize;
        for _ in 0..QUERIES_PER_SAMPLE {
            let result = context
                .cassie
                .execute_sql(&context.session, sql, vec![])
                .expect("execute Tier 2 column scan");
            assert_eq!(result.rows.len(), 100);
            completed_rows = completed_rows.saturating_add(result.rows.len());
        }
        u64::try_from(completed_rows).expect("result cardinality should fit u64")
    });
    let after = context.cassie.metrics();
    assert!(
        after["column_batches"]["scans"]
            .as_u64()
            .unwrap_or_default()
            > before["column_batches"]["scans"]
                .as_u64()
                .unwrap_or_default()
    );
    assert_eq!(
        after["column_batches"]["fallback_scans"]
            .as_u64()
            .unwrap_or_default(),
        before["column_batches"]["fallback_scans"]
            .as_u64()
            .unwrap_or_default()
    );
}
