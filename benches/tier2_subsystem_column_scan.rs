#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
#[path = "support/workloads.rs"]
mod workloads;

const ROWS: usize = 2_048;
const QUERIES_PER_SAMPLE: usize = 8;
const ALP_QUERIES_PER_SAMPLE: usize = 128;
const FSST_QUERIES_PER_SAMPLE: usize = 1_024;
const FIXTURE_ID: &str = "tier2_subsystem_column_scan/2k";
const COMPRESSIBLE_CANDIDATE: &str = "perf.column.selective_encoded_scan.2k";
const COMPRESSIBLE_BASELINE: &str = "perf.column.selective_plain_scan_baseline.2k";
const INCOMPRESSIBLE_CANDIDATE: &str = "perf.column.incompressible_adaptive_scan.2k";
const INCOMPRESSIBLE_BASELINE: &str = "perf.column.incompressible_plain_scan_baseline.2k";
const ALP_CANDIDATE: &str = "perf.column.alp_selective_scan.2k";
const ALP_BASELINE: &str = "perf.column.alp_plain_scan_baseline.2k";
const FSST_CANDIDATE: &str = "perf.column.fsst_selective_scan.2k";
const FSST_BASELINE: &str = "perf.column.fsst_plain_scan_baseline.2k";

#[derive(Clone, Copy)]
struct Scenario {
    workload: &'static str,
    sql: &'static str,
    queries_per_sample: usize,
    expected_rows: usize,
}

const SCENARIOS: [Scenario; 8] = [
    Scenario {
        workload: "selective_encoded_scan",
        sql: workloads::COMPRESSIBLE_AUTO_SQL,
        queries_per_sample: QUERIES_PER_SAMPLE,
        expected_rows: 100,
    },
    Scenario {
        workload: "selective_plain_scan_baseline",
        sql: workloads::COMPRESSIBLE_PLAIN_SQL,
        queries_per_sample: QUERIES_PER_SAMPLE,
        expected_rows: 100,
    },
    Scenario {
        workload: "incompressible_adaptive_scan",
        sql: workloads::INCOMPRESSIBLE_AUTO_SQL,
        queries_per_sample: QUERIES_PER_SAMPLE,
        expected_rows: 100,
    },
    Scenario {
        workload: "incompressible_plain_scan_baseline",
        sql: workloads::INCOMPRESSIBLE_PLAIN_SQL,
        queries_per_sample: QUERIES_PER_SAMPLE,
        expected_rows: 100,
    },
    Scenario {
        workload: "alp_selective_scan",
        sql: workloads::ALP_AUTO_SQL,
        queries_per_sample: ALP_QUERIES_PER_SAMPLE,
        expected_rows: 100,
    },
    Scenario {
        workload: "alp_plain_scan_baseline",
        sql: workloads::ALP_PLAIN_SQL,
        queries_per_sample: ALP_QUERIES_PER_SAMPLE,
        expected_rows: 100,
    },
    Scenario {
        workload: "fsst_selective_scan",
        sql: workloads::FSST_AUTO_SQL,
        queries_per_sample: FSST_QUERIES_PER_SAMPLE,
        expected_rows: 8,
    },
    Scenario {
        workload: "fsst_plain_scan_baseline",
        sql: workloads::FSST_PLAIN_SQL,
        queries_per_sample: FSST_QUERIES_PER_SAMPLE,
        expected_rows: 8,
    },
];

fn main() {
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier2,
        "tier2_subsystem_column_scan",
    );
    let measurements = SCENARIOS
        .into_iter()
        .map(|scenario| {
            let case = case(scenario.workload);
            let selected = runner.is_enabled(&case);
            (scenario, case, selected)
        })
        .collect::<Vec<_>>();
    let selections = measurements
        .iter()
        .map(|(_, _, selected)| *selected)
        .collect::<Vec<_>>();
    if selections.iter().any(|selected| *selected) {
        let setup_started = std::time::Instant::now();
        let runtime = workloads::runtime();
        let context = runtime
            .block_on(workloads::column_codec_acceptance_context(ROWS))
            .expect("prepare Tier 2 column codec acceptance fixture");
        let setup_time = setup_started.elapsed().as_nanos().max(1).to_string();

        for (scenario, case, selected) in measurements {
            measure_selected(&mut runner, &context, case, selected, &setup_time, scenario);
        }
        if selections[0] && selections[1] {
            runner.require_relative_p95(COMPRESSIBLE_CANDIDATE, COMPRESSIBLE_BASELINE, 0.85);
        }
        if selections[2] && selections[3] {
            runner.require_relative_p95(INCOMPRESSIBLE_CANDIDATE, INCOMPRESSIBLE_BASELINE, 1.05);
        }
        if selections[4] && selections[5] {
            runner.require_relative_p95(ALP_CANDIDATE, ALP_BASELINE, 1.05);
        }
        if selections[6] && selections[7] {
            runner.require_relative_p95(FSST_CANDIDATE, FSST_BASELINE, 0.85);
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
    setup_time: &str,
    scenario: Scenario,
) {
    if !selected {
        return;
    }
    let before = context.cassie.metrics();
    runner.measure_counted(case.metadata("setup_time_ns", setup_time), || {
        let mut completed_rows = 0usize;
        for _ in 0..scenario.queries_per_sample {
            let result = context
                .cassie
                .execute_sql(&context.session, scenario.sql, vec![])
                .expect("execute Tier 2 column scan");
            assert_eq!(result.rows.len(), scenario.expected_rows);
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
