use std::time::Instant;

use cassie_bench_support::{performance_benchmarks, stress};
use cassie_core::request::{Interruption, RequestCancellation, RequestMemory};

const RESERVATIONS_PER_SAMPLE: u64 = 256;
const CANCELLATION_CHECKS_PER_SAMPLE: u64 = 256;
const RESERVATION_BYTES: usize = 128;

fn main() {
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier1,
        "tier1_hotpath_request_controls",
    );
    let memory_case = stress::StressCase::new("request_memory_reserve_release", "micro")
        .runtime_contract(
            stress::FixtureDeclaration::new(
                performance_benchmarks::FixtureClass::Kernel,
                0,
                "tier1_hotpath_request_controls/micro",
            ),
            stress::OperationUnit::Request,
        );
    if runner.is_enabled(&memory_case) {
        let setup_started = Instant::now();
        let memory = RequestMemory::new(RESERVATION_BYTES);
        runner.measure_micro_batch(
            memory_case
                .metadata(
                    "setup_time_ns",
                    setup_started.elapsed().as_nanos().max(1).to_string(),
                )
                .parameter("reservation_bytes", RESERVATION_BYTES.to_string()),
            RESERVATIONS_PER_SAMPLE,
            || {
                let mut reserved_bytes = 0_usize;
                for _ in 0..RESERVATIONS_PER_SAMPLE {
                    let reservation = memory
                        .reserve(RESERVATION_BYTES)
                        .expect("reservation fits configured request budget");
                    reserved_bytes = reserved_bytes.saturating_add(reservation.bytes());
                    drop(reservation);
                }
                std::hint::black_box(reserved_bytes);
                RESERVATIONS_PER_SAMPLE
            },
        );
    }

    let cancellation_case = stress::StressCase::new("request_cancellation_check", "micro")
        .runtime_contract(
            stress::FixtureDeclaration::new(
                performance_benchmarks::FixtureClass::Kernel,
                0,
                "tier1_hotpath_request_controls/micro",
            ),
            stress::OperationUnit::Cancel,
        );
    if runner.is_enabled(&cancellation_case) {
        let setup_started = Instant::now();
        let cancellation = RequestCancellation::new();
        let interruption = Interruption::new(None, &cancellation);
        runner.measure_micro_batch(
            cancellation_case.metadata(
                "setup_time_ns",
                setup_started.elapsed().as_nanos().max(1).to_string(),
            ),
            CANCELLATION_CHECKS_PER_SAMPLE,
            || {
                for _ in 0..CANCELLATION_CHECKS_PER_SAMPLE {
                    interruption
                        .check()
                        .expect("uncanceled request controls should pass");
                    std::hint::black_box(cancellation.is_canceled());
                }
                CANCELLATION_CHECKS_PER_SAMPLE
            },
        );
    }
    runner.finish();
}
