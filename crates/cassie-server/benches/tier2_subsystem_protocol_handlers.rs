use std::time::Instant;

use cassie::benchmark::{
    PgwireFrontendCodecKernel, PgwireParameterBindingKernel, PgwireRowCodecKernel,
};
use cassie_bench_support::{performance_benchmarks, stress};

const VALUES: usize = 512;
const TARGET: &str = "tier2_subsystem_protocol_handlers";

fn main() {
    let mut runner = stress::runner(performance_benchmarks::BenchmarkTier::Tier2, TARGET);
    let frames = declared_case("frontend_frame_decode", stress::OperationUnit::Message);
    let parameters = declared_case("bind_parameter_decode", stress::OperationUnit::Parameter);
    let responses = declared_case("response_row_encoding", stress::OperationUnit::Message);
    let selected = [
        runner.is_enabled(&frames),
        runner.is_enabled(&parameters),
        runner.is_enabled(&responses),
    ];
    if !selected.into_iter().any(|enabled| enabled) {
        runner.finish();
        return;
    }
    let setup_started = Instant::now();
    let frame_fixture = PgwireFrontendCodecKernel::with_frames(VALUES);
    let parameter_fixture = PgwireParameterBindingKernel::with_parameters(VALUES);
    let response_fixtures = (0..VALUES)
        .map(PgwireRowCodecKernel::sample)
        .collect::<Vec<_>>();
    assert_eq!(frame_fixture.frame_count(), VALUES);
    assert_eq!(parameter_fixture.parameter_count(), VALUES);
    let setup_time_ns = setup_started.elapsed().as_nanos().to_string();

    if selected[0] {
        runner.measure_counted(frames.metadata("setup_time_ns", &setup_time_ns), || {
            let decoded = frame_fixture.decode();
            assert!(decoded > 0);
            u64::try_from(VALUES).expect("frame count fits u64")
        });
    }
    if selected[1] {
        runner.measure_counted(parameters.metadata("setup_time_ns", &setup_time_ns), || {
            let decoded = parameter_fixture.decode();
            assert_eq!(decoded.len(), VALUES);
            u64::try_from(decoded.len()).expect("parameter count fits u64")
        });
    }
    if selected[2] {
        runner.measure_counted(responses.metadata("setup_time_ns", &setup_time_ns), || {
            let encoded = response_fixtures
                .iter()
                .map(PgwireRowCodecKernel::encode)
                .map(|bytes| std::hint::black_box(bytes.len()))
                .sum::<usize>();
            std::hint::black_box(encoded);
            u64::try_from(VALUES).expect("response count fits u64")
        });
    }
    runner.finish();
}

fn declared_case(workload: &str, unit: stress::OperationUnit) -> stress::StressCase {
    stress::StressCase::new(workload, "512").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Subsystem,
            VALUES,
            format!("{TARGET}/512"),
        ),
        unit,
    )
}
