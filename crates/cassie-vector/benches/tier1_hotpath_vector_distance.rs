use std::time::Instant;

use cassie_bench_support::{performance_benchmarks, stress};
use cassie_vector::{cosine_distance, dot_score, l2_distance};

const DIMENSIONS: usize = 384;
const BATCH_SIZE: u64 = 4_096;
type VectorPair = ([f32; DIMENSIONS], [f32; DIMENSIONS]);

fn main() {
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier1,
        "tier1_hotpath_vector_distance",
    );
    let cosine = declared_case("cosine_distance");
    let dot = declared_case("dot_product");
    let l2 = declared_case("l2_distance");
    for (case, inputs_fn, operation) in [
        (
            cosine,
            cosine_distance_inputs as fn() -> [VectorPair; 32],
            cosine_distance as fn(&[f32], &[f32]) -> f64,
        ),
        (dot, dot_l2_inputs, dot_score),
        (l2, dot_l2_inputs, l2_distance),
    ] {
        if runner.is_enabled(&case) {
            let setup_started = Instant::now();
            let inputs = inputs_fn();
            runner.measure_micro_batch(
                case.metadata(
                    "setup_time_ns",
                    setup_started.elapsed().as_nanos().max(1).to_string(),
                )
                .metadata("micro_validation", "varied_inputs_accumulated_output")
                .parameter("vector_dimensions", DIMENSIONS.to_string()),
                BATCH_SIZE,
                || distance_batch(&inputs, operation),
            );
        }
    }
    runner.finish();
}

fn declared_case(workload: &str) -> stress::StressCase {
    stress::StressCase::new(workload, "micro").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Kernel,
            0,
            "tier1_hotpath_vector_distance/micro",
        ),
        stress::OperationUnit::Distance,
    )
}

fn cosine_distance_inputs() -> [VectorPair; 32] {
    std::array::from_fn(|index| {
        let shift = f32::from(u16::try_from(index).expect("input index fits u16")) / 1_000.0;
        (
            std::array::from_fn(|dimension| {
                0.01 + shift
                    + f32::from(u16::try_from(dimension % 17).expect("dimension fits u16")) / 17.0
            }),
            std::array::from_fn(|dimension| {
                0.02 + shift
                    + f32::from(u16::try_from((dimension * 7) % 19).expect("dimension fits u16"))
                        / 19.0
            }),
        )
    })
}

fn dot_l2_inputs() -> [VectorPair; 32] {
    std::array::from_fn(|index| {
        let shift = f32::from(u16::try_from(index).expect("input index fits u16")) / 1_000.0;
        (
            std::array::from_fn(|dimension| {
                0.03 + shift
                    + f32::from(u16::try_from(dimension % 23).expect("dimension fits u16")) / 23.0
            }),
            std::array::from_fn(|dimension| {
                0.04 + shift
                    + f32::from(u16::try_from((dimension * 11) % 29).expect("dimension fits u16"))
                        / 29.0
            }),
        )
    })
}

fn distance_batch(inputs: &[VectorPair; 32], operation: fn(&[f32], &[f32]) -> f64) -> u64 {
    let rounds = BATCH_SIZE / u64::try_from(inputs.len()).expect("fixture count fits u64");
    let mut accumulated_distance = 0.0;
    for _ in 0..rounds {
        for (left, right) in std::hint::black_box(inputs) {
            accumulated_distance +=
                operation(std::hint::black_box(left), std::hint::black_box(right));
        }
    }
    std::hint::black_box(accumulated_distance);
    rounds * u64::try_from(inputs.len()).expect("fixture count fits u64")
}
