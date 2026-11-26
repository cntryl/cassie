use std::time::Instant;

use cassie_bench_support::{performance_benchmarks, stress};
use cassie_sql::parser::parse_statement;

const PARSER_INVOCATIONS_PER_SAMPLE: usize = 8;
const PLANNING_SQL: &str = "SELECT id, title FROM bench_documents WHERE score >= $1 AND status = 'approved' ORDER BY id LIMIT 20";

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
        let setup_started = Instant::now();
        let fixture = ParserFixture::new(128);
        runner.measure_counted_batch(
            case.metadata(
                "setup_time_ns",
                setup_started.elapsed().as_nanos().max(1).to_string(),
            ),
            PARSER_INVOCATIONS_PER_SAMPLE,
            || fixture.parse(),
        );
    }

    for operator_count in [8, 16, 32, 64, 128] {
        let fixture_scale = format!("{operator_count}-operators");
        let case = stress::StressCase::new("sql_parser_qualified_operators", fixture_scale)
            .parameter("operators_per_expression", operator_count.to_string())
            .runtime_contract(
                stress::FixtureDeclaration::new(
                    performance_benchmarks::FixtureClass::Subsystem,
                    1,
                    format!("tier2_subsystem_parser/qualified_operators/{operator_count}"),
                ),
                stress::OperationUnit::Statement,
            );
        if runner.is_enabled(&case) {
            let setup_started = Instant::now();
            let fixture = QualifiedOperatorParserFixture::new(operator_count);
            runner.measure_counted_batch(
                case.parameter("statement_bytes", fixture.statement.len().to_string())
                    .metadata(
                        "setup_time_ns",
                        setup_started.elapsed().as_nanos().max(1).to_string(),
                    ),
                PARSER_INVOCATIONS_PER_SAMPLE,
                || fixture.parse(),
            );
        }
    }
    runner.finish();
}

struct ParserFixture {
    statements: Vec<&'static str>,
}

impl ParserFixture {
    fn new(count: usize) -> Self {
        Self {
            statements: vec![PLANNING_SQL; count],
        }
    }

    fn parse(&self) -> u64 {
        let parsed = self
            .statements
            .iter()
            .map(|sql| parse_statement(sql).expect("benchmark SQL should parse"))
            .collect::<Vec<_>>();
        std::hint::black_box(parsed);
        u64::try_from(self.statements.len()).expect("statement count fits u64")
    }
}

struct QualifiedOperatorParserFixture {
    statement: String,
}

impl QualifiedOperatorParserFixture {
    fn new(operator_count: usize) -> Self {
        assert!((1..=2_048).contains(&operator_count));
        let mut statement = String::from("SELECT 1");
        for _ in 0..operator_count {
            statement.push_str(" OPERATOR(pg_catalog.+) 1");
        }
        Self { statement }
    }

    fn parse(&self) -> u64 {
        let parsed = parse_statement(&self.statement)
            .expect("qualified-operator benchmark SQL should parse");
        std::hint::black_box(parsed);
        1
    }
}
