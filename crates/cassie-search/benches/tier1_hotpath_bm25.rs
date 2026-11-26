use std::time::Instant;

use cassie_bench_support::{performance_benchmarks, stress};
use cassie_search::bm25_score;

const BM25_BATCH_SIZE: u64 = 256;
const BM25_TERMS_PER_SCORE: u64 = 8;
type Bm25Input = (f64, f64, f64, f64, f64);
const BM25_INPUTS: [Bm25Input; 8] = [
    (3.0, 10.0, 1_000.0, 120.0, 100.0),
    (4.0, 11.0, 1_000.0, 118.0, 100.0),
    (5.0, 12.0, 1_000.0, 116.0, 100.0),
    (6.0, 13.0, 1_000.0, 114.0, 100.0),
    (7.0, 14.0, 1_000.0, 112.0, 100.0),
    (8.0, 15.0, 1_000.0, 110.0, 100.0),
    (9.0, 16.0, 1_000.0, 108.0, 100.0),
    (10.0, 17.0, 1_000.0, 106.0, 100.0),
];

fn main() {
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier1,
        "tier1_hotpath_bm25",
    );
    let case = stress::StressCase::new("bm25_scoring", "micro").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Kernel,
            0,
            "tier1_hotpath_bm25/micro",
        ),
        stress::OperationUnit::Score,
    );
    if runner.is_enabled(&case) {
        let setup_started = Instant::now();
        assert_eq!(
            u64::try_from(BM25_INPUTS.len()).expect("term count fits u64"),
            BM25_TERMS_PER_SCORE
        );
        runner.measure_micro_batch(
            case.metadata(
                "setup_time_ns",
                setup_started.elapsed().as_nanos().max(1).to_string(),
            )
            .metadata("micro_validation", "varied_inputs_accumulated_output")
            .parameter("terms_per_score", BM25_TERMS_PER_SCORE.to_string()),
            BM25_BATCH_SIZE,
            bm25_score_batch,
        );
    }
    runner.finish();
}

fn bm25_score_batch() -> u64 {
    let mut accumulated_score = 0.0;
    for _ in 0..BM25_BATCH_SIZE {
        let score = BM25_INPUTS
            .iter()
            .map(|&(tf, df, n, dl, avgdl)| {
                bm25_score(
                    std::hint::black_box(tf),
                    std::hint::black_box(df),
                    std::hint::black_box(n),
                    std::hint::black_box(1.2),
                    std::hint::black_box(0.75),
                    std::hint::black_box(dl),
                    std::hint::black_box(avgdl),
                )
            })
            .sum::<f64>();
        accumulated_score += std::hint::black_box(score);
    }
    std::hint::black_box(accumulated_score);
    BM25_BATCH_SIZE
}
