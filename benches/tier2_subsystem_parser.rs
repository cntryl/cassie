#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier2,
        "tier2_subsystem_parser",
    );
    let case = stress::StressCase::new("sql_parser", "128").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Subsystem,
            128,
            "tier2_subsystem_parser/128",
        ),
        stress::OperationUnit::Statement,
    );
    if runner.is_enabled(&case) {
        let setup_started = std::time::Instant::now();
        let fixture = workloads::ParserFixture::new(128);
        runner.measure_counted_batch(
            case.metadata(
                "setup_time_ns",
                setup_started.elapsed().as_nanos().to_string(),
            ),
            workloads::PARSER_INVOCATIONS_PER_SAMPLE,
            || fixture.parse(),
        );
    }
    for operator_count in [8, 32, 64] {
        let fixture_scale = format!("{operator_count}-operators");
        let qualified_case =
            stress::StressCase::new("sql_parser_qualified_operators", fixture_scale)
                .parameter("operators_per_expression", operator_count.to_string())
                .runtime_contract(
                    stress::FixtureDeclaration::new(
                        performance_benchmarks::FixtureClass::Subsystem,
                        1,
                        format!("tier2_subsystem_parser/qualified_operators/{operator_count}"),
                    ),
                    stress::OperationUnit::Statement,
                );
        if runner.is_enabled(&qualified_case) {
            let setup_started = std::time::Instant::now();
            let fixture = workloads::QualifiedOperatorParserFixture::new(operator_count);
            runner.measure_counted_batch(
                qualified_case.metadata(
                    "setup_time_ns",
                    setup_started.elapsed().as_nanos().to_string(),
                ),
                workloads::PARSER_INVOCATIONS_PER_SAMPLE,
                || fixture.parse(),
            );
        }
    }
    runner.finish();
}
