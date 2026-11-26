use std::time::Instant;

use cassie_bench_support::{performance_benchmarks, stress};
use cassie_core::types::{DataType, FieldSchema, Schema};
use cassie_storage::{
    column_batch_format_v1::{decode_column_chunk, encode_column_chunk, LogicalType},
    row_blob::{decode_row, encode_row, RowSchema},
};
use serde_json::json;

const ROWS_PER_SAMPLE: u64 = 64;
const COLUMN_VALUES: usize = 1_024;

fn main() {
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier1,
        "tier1_hotpath_row_codec",
    );
    measure_rows(&mut runner);

    for (workload, logical_type, values) in [
        (
            "column_codec_encode",
            LogicalType::Int64,
            (0..COLUMN_VALUES)
                .map(|i| json!(i % 16))
                .collect::<Vec<_>>(),
        ),
        (
            "alp_codec_encode",
            LogicalType::Float64,
            (0..COLUMN_VALUES)
                .map(|i| {
                    let value = f64::from(u16::try_from(i).expect("column index fits u16"));
                    json!((value - 512.0) / 100.0)
                })
                .collect(),
        ),
        (
            "fsst_codec_encode",
            LogicalType::Utf8,
            (0..COLUMN_VALUES)
                .map(|i| json!(format!("tenant-{}/event-{i}-payload-{}", i % 32, i % 8)))
                .collect(),
        ),
    ] {
        let case = stress::StressCase::new(workload, "micro").runtime_contract(
            stress::FixtureDeclaration::new(
                performance_benchmarks::FixtureClass::Kernel,
                0,
                "tier1_hotpath_row_codec/micro",
            ),
            stress::OperationUnit::Operation,
        );
        let encoded = encode_column_chunk(logical_type, &values).expect("encode column fixture");
        if workload == "alp_codec_encode" {
            assert_eq!(encoded.codec.name(), "alp");
        } else if workload == "fsst_codec_encode" {
            assert_eq!(encoded.codec.name(), "fsst");
        }
        let bytes = encoded.bytes;
        assert_eq!(
            decode_column_chunk(&bytes)
                .expect("decode column fixture")
                .values
                .len(),
            values.len()
        );
        if runner.is_enabled(&case) {
            runner.measure_micro(case, || {
                let result =
                    encode_column_chunk(logical_type, &values).expect("encode benchmark column");
                std::hint::black_box(result.bytes.len())
            });
        }
        let decode_workload = workload.replace("_encode", "_decode");
        let decode_case = stress::StressCase::new(decode_workload, "micro").runtime_contract(
            stress::FixtureDeclaration::new(
                performance_benchmarks::FixtureClass::Kernel,
                0,
                "tier1_hotpath_row_codec/micro",
            ),
            stress::OperationUnit::Operation,
        );
        if runner.is_enabled(&decode_case) {
            runner.measure_micro(decode_case, || {
                let decoded = decode_column_chunk(&bytes).expect("decode benchmark column");
                std::hint::black_box(decoded.values.len())
            });
        }
    }
    runner.finish();
}

fn measure_rows(runner: &mut stress::CassieStressRunner) {
    let row_case = stress::StressCase::new("row_encode_decode", "micro").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Kernel,
            0,
            "tier1_hotpath_row_codec/micro",
        ),
        stress::OperationUnit::Row,
    );
    let started = Instant::now();
    let schema = row_schema();
    let row = json!({"id": "doc-1", "title": "alpha", "score": 42});
    let encoded_row = encode_row(&schema, &row).expect("encode Tier 1 row fixture");
    assert_eq!(
        decode_row(&schema, &encoded_row, decode_fixture_json).expect("decode row fixture"),
        row
    );
    let setup_time_ns = started.elapsed().as_nanos().max(1).to_string();
    if runner.is_enabled(&row_case) {
        runner.measure_micro_batch(
            row_case.metadata("setup_time_ns", setup_time_ns),
            ROWS_PER_SAMPLE,
            || {
                for _ in 0..ROWS_PER_SAMPLE {
                    let bytes = encode_row(&schema, &row).expect("encode benchmark row");
                    std::hint::black_box(
                        decode_row(&schema, &bytes, decode_fixture_json)
                            .expect("decode benchmark row"),
                    );
                }
                ROWS_PER_SAMPLE
            },
        );
    }
}

fn decode_fixture_json(bytes: &[u8]) -> serde_json::Result<serde_json::Value> {
    serde_json::from_slice(bytes)
}

fn row_schema() -> RowSchema {
    RowSchema::from_schema(&Schema {
        fields: vec![
            FieldSchema {
                name: "id".to_owned(),
                data_type: DataType::Text,
                nullable: false,
            },
            FieldSchema {
                name: "title".to_owned(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "score".to_owned(),
                data_type: DataType::Int,
                nullable: true,
            },
        ],
    })
}
