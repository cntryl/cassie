#[path = "support/subsystem.rs"]
mod workloads;

use std::time::Instant;

use cassie_bench_support::{performance_benchmarks, stress};
use cassie_engine::{
    catalog::Catalog,
    config::CassieRuntimeLimits,
    runtime::QueryExecutionControls,
    sql::{self, ast::ParsedStatement},
};

const ADMISSION_INVOCATIONS_PER_SAMPLE: usize = 8_192;
const PARAMETERS: usize = 128;

fn main() {
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier2,
        "tier2_subsystem_binder",
    );
    let cases = [
        ("sql_binder", stress::OperationUnit::Statement),
        ("parameter_type_admission", stress::OperationUnit::Parameter),
        ("logical_planner", stress::OperationUnit::Plan),
        ("physical_planner", stress::OperationUnit::Plan),
    ]
    .map(|(workload, unit)| declared_case(workload, unit));
    let enabled = cases.each_ref().map(|case| runner.is_enabled(case));
    if enabled.into_iter().any(|is_enabled| is_enabled) {
        if enabled[0] {
            let start = Instant::now();
            let fixture = workloads::BindingFixture::new(128);
            runner.measure_counted_batch(
                with_setup(cases[0].clone(), start.elapsed().as_nanos()),
                workloads::PLANNING_FIXTURE_INVOCATIONS_PER_SAMPLE,
                || fixture.bind(),
            );
        }
        if enabled[1] {
            let start = Instant::now();
            let fixture = ParameterAdmissionFixture::new();
            runner.measure_counted_batch(
                with_setup(cases[1].clone(), start.elapsed().as_nanos()),
                ADMISSION_INVOCATIONS_PER_SAMPLE,
                || fixture.admit(),
            );
        }
        if enabled[2] {
            let start = Instant::now();
            let fixture = workloads::LogicalPlanningFixture::new(128);
            runner.measure_counted_batch(
                with_setup(cases[2].clone(), start.elapsed().as_nanos()),
                workloads::PLANNING_FIXTURE_INVOCATIONS_PER_SAMPLE,
                || fixture.logical_plan(),
            );
        }
        if enabled[3] {
            let start = Instant::now();
            let fixture = workloads::PhysicalPlanningFixture::new(128);
            runner.measure_counted_batch(
                with_setup(cases[3].clone(), start.elapsed().as_nanos()),
                workloads::PLANNING_FIXTURE_INVOCATIONS_PER_SAMPLE,
                || fixture.physical_plan(),
            );
        }
    }
    runner.finish();
}

fn with_setup(case: stress::StressCase, setup_time: u128) -> stress::StressCase {
    case.metadata("setup_time_ns", setup_time.to_string())
}

fn declared_case(workload: &str, unit: stress::OperationUnit) -> stress::StressCase {
    stress::StressCase::new(workload, "128").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Subsystem,
            128,
            "tier2_subsystem_binder/128",
        ),
        unit,
    )
}

struct ParameterAdmissionFixture {
    statement: ParsedStatement,
    declared_oids: Vec<i32>,
    catalog: Catalog,
    controls: QueryExecutionControls,
}

impl ParameterAdmissionFixture {
    fn new() -> Self {
        let types = [
            ("INTEGER", 23),
            ("TEXT", 25),
            ("BOOLEAN", 16),
            ("DOUBLE PRECISION", 701),
        ];
        let mut projections = Vec::with_capacity(PARAMETERS);
        let mut declared_oids = Vec::with_capacity(PARAMETERS);
        for index in 0..PARAMETERS {
            let (name, oid) = types[index % types.len()];
            projections.push(format!("${}::{name}", index + 1));
            declared_oids.push(oid);
        }
        let statement = sql::parse_statement(&format!("SELECT {}", projections.join(", ")))
            .expect("parameter admission benchmark SQL parses");
        let limits = CassieRuntimeLimits {
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        };
        Self {
            statement,
            declared_oids,
            catalog: Catalog::new(),
            controls: QueryExecutionControls::from_limits(&limits, Instant::now()),
        }
    }

    fn admit(&self) -> u64 {
        let admitted = sql::checked_wire_parse_parameter_types_in_scope(
            &self.statement,
            &self.declared_oids,
            &self.catalog,
            &self.controls,
            None,
        )
        .and_then(sql::AdmittedParameterTypes::into_vec)
        .expect("benchmark parameter types admit");
        assert_eq!(admitted, self.declared_oids);
        std::hint::black_box(admitted);
        u64::try_from(self.declared_oids.len()).expect("parameter count fits u64")
    }
}
