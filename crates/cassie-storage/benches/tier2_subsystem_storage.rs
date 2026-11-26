use std::time::Instant;

use cassie_bench_support::{performance_benchmarks, stress};
use cassie_storage::{
    adapter::{Midge, StorageFamily},
    key_codec,
};

const ROWS: usize = 1_024;
const WORKLOAD: &str = "tier2_subsystem_storage";

fn main() {
    let mut runner = stress::runner(performance_benchmarks::BenchmarkTier::Tier2, WORKLOAD);
    let lookup = declared_case("indexed_lookup", stress::OperationUnit::Lookup);
    let batch = declared_case("committed_write_batch", stress::OperationUnit::Batch);
    if !runner.is_enabled(&lookup) && !runner.is_enabled(&batch) {
        runner.finish();
        return;
    }
    let setup_started = Instant::now();
    let midge = Midge::new_in_memory("postgres").expect("open benchmark Midge");
    let mut tx = midge
        .begin_raw_family_adapter_tx(
            StorageFamily::Data,
            cntryl_midge::TransactionMode::ReadWrite,
        )
        .expect("begin indexed fixture write");
    for row in 0..ROWS {
        tx.put(index_key(row), index_value(row), None)
            .expect("stage indexed fixture row");
    }
    tx.commit(midge.write_options_sync())
        .expect("commit indexed fixture");
    let setup_time_ns = setup_started.elapsed().as_nanos().to_string();

    if runner.is_enabled(&lookup) {
        let lookup = declared_case("indexed_lookup", stress::OperationUnit::Lookup)
            .metadata("setup_time_ns", &setup_time_ns);
        let key = index_key(ROWS / 2);
        let expected = index_value(ROWS / 2);
        assert_eq!(
            midge
                .raw_get(StorageFamily::Data, &key)
                .expect("indexed preflight"),
            Some(expected.clone())
        );
        runner.measure_counted(lookup, || {
            let value = midge
                .raw_get(StorageFamily::Data, &key)
                .expect("indexed lookup");
            assert_eq!(value, Some(expected.clone()));
            1
        });
    }
    if runner.is_enabled(&batch) {
        let batch = declared_case("committed_write_batch", stress::OperationUnit::Batch)
            .metadata("setup_time_ns", &setup_time_ns);
        runner.measure_counted(batch, || {
            let mut tx = midge
                .begin_raw_family_adapter_tx(
                    StorageFamily::Data,
                    cntryl_midge::TransactionMode::ReadWrite,
                )
                .expect("begin committed benchmark batch");
            for row in 0..ROWS {
                tx.put(batch_key(row), index_value(row), None)
                    .expect("stage committed benchmark row");
            }
            tx.commit(midge.write_options_sync())
                .expect("commit benchmark batch");
            1
        });
    }
    runner.finish();
}

fn declared_case(workload: &str, unit: stress::OperationUnit) -> stress::StressCase {
    stress::StressCase::new(workload, "1k").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Subsystem,
            ROWS,
            "tier2_subsystem_storage/1k",
        ),
        unit,
    )
}

fn index_key(row: usize) -> Vec<u8> {
    key_codec::encode_key(b"benchmark-index", &[format!("{row:04}").as_bytes()])
}

fn batch_key(row: usize) -> Vec<u8> {
    key_codec::encode_key(b"benchmark-batch", &[format!("{row:04}").as_bytes()])
}

fn index_value(row: usize) -> Vec<u8> {
    format!("document-{row:04}").into_bytes()
}
