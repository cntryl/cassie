use std::time::Instant;

use cassie::benchmark::{
    PgwireFrontendCodecKernel, PgwireParameterBindingKernel, PgwireRowCodecKernel,
};
use cassie_bench_support::{performance_benchmarks, stress};

const VALUES: usize = 512;
const TARGET: &str = "tier2_subsystem_protocol_handlers";

fn main() {
    let mut runner = stress::runner(performance_benchmarks::BenchmarkTier::Tier2, TARGET);
    measure_codecs(&mut runner);

    let parameters = declared_case("bind_parameter_decode", stress::OperationUnit::Parameter);
    if runner.is_enabled(&parameters) {
        let setup_started = Instant::now();
        let fixture = PgwireParameterBindingKernel::with_parameters(VALUES);
        assert_eq!(fixture.parameter_count(), VALUES);
        let setup_time_ns = setup_started.elapsed().as_nanos().to_string();
        runner.measure_counted(parameters.metadata("setup_time_ns", setup_time_ns), || {
            let decoded = fixture.decode();
            assert_eq!(decoded.len(), VALUES);
            u64::try_from(decoded.len()).expect("parameter count fits u64")
        });
    }

    runner.finish();
}

fn measure_codecs(runner: &mut stress::CassieStressRunner) {
    let pgwire = declared_case("pgwire_codec", stress::OperationUnit::Message);
    let prepared = declared_case("prepared_statement_loop", stress::OperationUnit::Message);
    let json = declared_case("json_serialization", stress::OperationUnit::Row);
    let selected = [
        runner.is_enabled(&pgwire),
        runner.is_enabled(&prepared),
        runner.is_enabled(&json),
    ];
    if !selected.into_iter().any(|enabled| enabled) {
        return;
    }
    let setup_started = Instant::now();
    let pgwire_rows = (0..VALUES)
        .map(PgwireRowCodecKernel::sample)
        .collect::<Vec<_>>();
    let prepared_messages = PgwireFrontendCodecKernel::with_frames(VALUES);
    let json_rows = (0..VALUES)
        .map(|index| {
            serde_json::json!({
                "id": format!("doc-{index}"),
                "title": format!("title-{}", index % 16),
                "score": index % 100,
            })
        })
        .collect::<Vec<_>>();
    let setup_time_ns = setup_started.elapsed().as_nanos().to_string();

    if selected[0] {
        runner.measure_counted_batch(
            pgwire.metadata("setup_time_ns", &setup_time_ns),
            256,
            || {
                let encoded = pgwire_rows
                    .iter()
                    .map(PgwireRowCodecKernel::encode)
                    .collect::<Vec<_>>();
                std::hint::black_box(encoded);
                u64::try_from(pgwire_rows.len()).expect("pgwire row count fits u64")
            },
        );
    }
    if selected[1] {
        runner.measure_counted_batch(
            prepared.metadata("setup_time_ns", &setup_time_ns),
            1_024,
            || {
                std::hint::black_box(prepared_messages.decode());
                u64::try_from(prepared_messages.frame_count()).expect("frame count fits u64")
            },
        );
    }
    if selected[2] {
        runner.measure_counted_batch(json.metadata("setup_time_ns", &setup_time_ns), 16, || {
            let encoded = serde_json::to_vec(&json_rows).expect("benchmark rows should serialize");
            std::hint::black_box(encoded);
            u64::try_from(json_rows.len()).expect("JSON row count fits u64")
        });
    }
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
