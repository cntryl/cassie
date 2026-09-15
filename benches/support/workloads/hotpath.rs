#![allow(dead_code, unused_imports)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};
use std::thread;
use std::time::{Duration, Instant};

use cassie::app::{Cassie, CassieError, CassieSession};
use cassie::config::{
    CassieRuntimeConfig, EmbeddingsRuntimeConfig, SelfHostedEmbeddingRuntimeConfig,
};
use cassie::planner::{logical, physical};
use cassie::rest::{documents, search};
use cassie::runtime::ExecutionMode;
use cassie::search::{bm25, tokenizer};
use cassie::sql::{binder, parse_statement};
use cassie::types::{DataType, FieldSchema, Schema, Value};
use serde_json::json;

use super::context::{usize_to_f32, BenchContext, QueryBreakdownMicros};

static ROW_CODEC_KERNEL: LazyLock<cassie::benchmark::RowCodecKernel> =
    LazyLock::new(cassie::benchmark::RowCodecKernel::sample);
static ROW_KEY_KERNEL: LazyLock<cassie::benchmark::RowKeyKernel> =
    LazyLock::new(cassie::benchmark::RowKeyKernel::default);
static EXECUTOR_KERNEL: LazyLock<cassie::benchmark::ExecutorKernel> =
    LazyLock::new(cassie::benchmark::ExecutorKernel::sample);
static PGWIRE_ROW: LazyLock<cassie::benchmark::PgwireRowCodecKernel> =
    LazyLock::new(|| cassie::benchmark::PgwireRowCodecKernel::sample(1));
static COLUMN_CODEC_VALUES: LazyLock<Vec<serde_json::Value>> = LazyLock::new(|| {
    (0..1_024)
        .map(|index| serde_json::json!(index % 16))
        .collect()
});
static COLUMN_CODEC_BYTES: LazyLock<Vec<u8>> = LazyLock::new(|| {
    cassie::midge::adapter::encode_column_chunk_for_test("bigint", &COLUMN_CODEC_VALUES)
        .expect("encode Tier 1 column codec fixture")
});
static ALP_CODEC_VALUES: LazyLock<Vec<serde_json::Value>> = LazyLock::new(|| {
    (0..1_024)
        .map(|index| serde_json::json!((f64::from(index) - 512.0) / 100.0))
        .collect()
});
static ALP_CODEC_BYTES: LazyLock<Vec<u8>> = LazyLock::new(|| {
    let encoded = cassie::midge::adapter::encode_column_chunk_for_test("float", &ALP_CODEC_VALUES)
        .expect("encode Tier 1 ALP codec fixture");
    assert_eq!(
        cassie::midge::adapter::column_chunk_codec_for_test(&encoded)
            .expect("inspect Tier 1 ALP codec fixture"),
        "alp"
    );
    encoded
});
static FSST_CODEC_VALUES: LazyLock<Vec<serde_json::Value>> = LazyLock::new(|| {
    (0..1_024)
        .map(|position| {
            serde_json::json!(format!(
                "tenant-{}/event-{}-payload-{}",
                position % 32,
                position,
                position % 8
            ))
        })
        .collect()
});
static FSST_CODEC_BYTES: LazyLock<Vec<u8>> = LazyLock::new(|| {
    let encoded = cassie::midge::adapter::encode_column_chunk_for_test("text", &FSST_CODEC_VALUES)
        .expect("encode Tier 1 FSST codec fixture");
    assert_eq!(
        cassie::midge::adapter::column_chunk_codec_for_test(&encoded)
            .expect("inspect Tier 1 FSST codec fixture"),
        "fsst"
    );
    encoded
});

pub const VECTOR_DISTANCE_DIMENSIONS: usize = 384;
type VectorPair = (
    [f32; VECTOR_DISTANCE_DIMENSIONS],
    [f32; VECTOR_DISTANCE_DIMENSIONS],
);
type Bm25Input = (f64, f64, f64, f64, f64);

pub const ROW_CODEC_BATCH_SIZE: u64 = 64;
pub const KEY_CODEC_BATCH_SIZE: u64 = 64;
pub const PROJECTION_BATCH_SIZE: u64 = 64;
pub const SCALAR_EVALUATION_BATCH_SIZE: u64 = 256;
pub const PGWIRE_ROW_BATCH_SIZE: u64 = 64;
pub const TOP_K_BATCH_SIZE: u64 = 32;
pub const TOKENIZATION_BATCH_SIZE: u64 = 256;
pub const BM25_BATCH_SIZE: u64 = 256;
pub const BM25_TERMS_PER_SCORE: u64 = 8;
pub const VECTOR_DISTANCE_BATCH_SIZE: u64 = 4_096;

static TOKENIZATION_INPUTS: [&str; 8] = [
    "Alpha beta, gamma and delta",
    "Cassie plans selective indexed queries",
    "Midge persists ordered document keys",
    "Hybrid retrieval combines text and vectors",
    "PostgreSQL clients reuse prepared statements",
    "Column batches accelerate analytical filters",
    "Snapshots preserve catalog and row state",
    "Operators inspect bounded runtime metrics",
];

static BM25_INPUTS: [Bm25Input; 8] = [
    (3.0, 10.0, 1_000.0, 120.0, 100.0),
    (4.0, 11.0, 1_000.0, 118.0, 100.0),
    (5.0, 12.0, 1_000.0, 116.0, 100.0),
    (6.0, 13.0, 1_000.0, 114.0, 100.0),
    (7.0, 14.0, 1_000.0, 112.0, 100.0),
    (8.0, 15.0, 1_000.0, 110.0, 100.0),
    (9.0, 16.0, 1_000.0, 108.0, 100.0),
    (10.0, 17.0, 1_000.0, 106.0, 100.0),
];

static COSINE_INPUTS: LazyLock<[VectorPair; 32]> = LazyLock::new(|| {
    std::array::from_fn(|index| {
        let shift = usize_to_f32(index) / 1_000.0;
        (
            std::array::from_fn(|dimension| 0.01 + shift + usize_to_f32(dimension % 17) / 17.0),
            std::array::from_fn(|dimension| {
                0.02 + shift + usize_to_f32((dimension * 7) % 19) / 19.0
            }),
        )
    })
});
static DOT_L2_INPUTS: LazyLock<[VectorPair; 32]> = LazyLock::new(|| {
    std::array::from_fn(|index| {
        let shift = usize_to_f32(index) / 1_000.0;
        (
            std::array::from_fn(|dimension| 0.03 + shift + usize_to_f32(dimension % 23) / 23.0),
            std::array::from_fn(|dimension| {
                0.04 + shift + usize_to_f32((dimension * 11) % 29) / 29.0
            }),
        )
    })
});

pub fn prepare_hotpath(workload: &str) -> Result<(), &'static str> {
    match workload {
        "row_encode_decode" => {
            LazyLock::force(&ROW_CODEC_KERNEL);
        }
        "column_codec_encode" => {
            LazyLock::force(&COLUMN_CODEC_VALUES);
        }
        "column_codec_decode" => {
            LazyLock::force(&COLUMN_CODEC_BYTES);
        }
        "alp_codec_encode" | "alp_codec_decode" => {
            LazyLock::force(&ALP_CODEC_BYTES);
        }
        "fsst_codec_encode" | "fsst_codec_decode" => {
            LazyLock::force(&FSST_CODEC_BYTES);
        }
        "key_encode_decode" => {
            LazyLock::force(&ROW_KEY_KERNEL);
        }
        "batch_filter"
        | "batch_projection"
        | "value_comparison"
        | "predicate_evaluation"
        | "top_k_heap_maintenance" => {
            LazyLock::force(&EXECUTOR_KERNEL);
        }
        "row_to_pgwire_encoding" => {
            LazyLock::force(&PGWIRE_ROW);
        }
        "cosine_distance" => {
            LazyLock::force(&COSINE_INPUTS);
        }
        "dot_product" | "l2_distance" => {
            LazyLock::force(&DOT_L2_INPUTS);
        }
        "tokenization" => {}
        "bm25_scoring" => {
            assert_eq!(
                u64::try_from(BM25_INPUTS.len()).expect("BM25 term count should fit"),
                BM25_TERMS_PER_SCORE,
                "BM25 score term declaration must match the fixture"
            );
        }
        _ => return Err("unknown Tier 1 hot-path workload"),
    }
    Ok(())
}

pub fn row_encode_decode_batch() -> u64 {
    let mut observed_bytes = 0_usize;
    for _ in 0..ROW_CODEC_BATCH_SIZE {
        let (encoded, decoded) = ROW_CODEC_KERNEL.round_trip();
        observed_bytes = observed_bytes.saturating_add(encoded.len());
        std::hint::black_box((encoded, decoded));
    }
    std::hint::black_box(observed_bytes);
    ROW_CODEC_BATCH_SIZE
}

pub fn column_codec_encode() -> usize {
    let encoded =
        cassie::midge::adapter::encode_column_chunk_for_test("bigint", &COLUMN_CODEC_VALUES)
            .expect("encode Tier 1 column chunk");
    std::hint::black_box(encoded).len()
}

pub fn column_codec_decode() -> usize {
    let decoded = cassie::midge::adapter::decode_column_chunk_for_test(&COLUMN_CODEC_BYTES)
        .expect("decode Tier 1 column chunk");
    std::hint::black_box(decoded).len()
}

pub fn alp_codec_encode() -> usize {
    let encoded = cassie::midge::adapter::encode_column_chunk_for_test("float", &ALP_CODEC_VALUES)
        .expect("encode Tier 1 ALP column chunk");
    std::hint::black_box(encoded).len()
}

pub fn alp_codec_decode() -> usize {
    let decoded = cassie::midge::adapter::decode_column_chunk_for_test(&ALP_CODEC_BYTES)
        .expect("decode Tier 1 ALP column chunk");
    std::hint::black_box(decoded).len()
}

pub fn fsst_codec_encode() -> usize {
    let encoded = cassie::midge::adapter::encode_column_chunk_for_test("text", &FSST_CODEC_VALUES)
        .expect("encode Tier 1 FSST column chunk");
    std::hint::black_box(encoded).len()
}

pub fn fsst_codec_decode() -> usize {
    let decoded = cassie::midge::adapter::decode_column_chunk_for_test(&FSST_CODEC_BYTES)
        .expect("decode Tier 1 FSST column chunk");
    std::hint::black_box(decoded).len()
}

pub fn key_encode_decode_batch() -> u64 {
    let mut observed_bytes = 0_usize;
    for _ in 0..KEY_CODEC_BATCH_SIZE {
        let (encoded, decoded) = ROW_KEY_KERNEL.encode_decode();
        observed_bytes = observed_bytes.saturating_add(encoded.len());
        std::hint::black_box((encoded, decoded));
    }
    std::hint::black_box(observed_bytes);
    KEY_CODEC_BATCH_SIZE
}

pub fn predicate_evaluation_batch() -> u64 {
    let mut matched = 0_usize;
    for _ in 0..SCALAR_EVALUATION_BATCH_SIZE {
        matched = matched.saturating_add(usize::from(EXECUTOR_KERNEL.predicate_matches()));
    }
    std::hint::black_box(matched);
    SCALAR_EVALUATION_BATCH_SIZE
}

pub fn batch_filter() -> usize {
    std::hint::black_box(EXECUTOR_KERNEL.filter_batch())
}

pub fn batch_projection_batch() -> u64 {
    let mut projected_values = 0_usize;
    for _ in 0..PROJECTION_BATCH_SIZE {
        projected_values = projected_values.saturating_add(EXECUTOR_KERNEL.project_row().len());
    }
    std::hint::black_box(projected_values);
    PROJECTION_BATCH_SIZE
}

pub fn value_comparison_batch() -> u64 {
    let mut matched = 0_usize;
    for _ in 0..SCALAR_EVALUATION_BATCH_SIZE {
        matched = matched.saturating_add(usize::from(EXECUTOR_KERNEL.value_comparison_matches()));
    }
    std::hint::black_box(matched);
    SCALAR_EVALUATION_BATCH_SIZE
}

pub fn top_k_update_batch() -> cassie::benchmark::KernelObservation {
    let mut result_cardinality = 0_u64;
    let mut candidate_count = 0_u64;
    for _ in 0..TOP_K_BATCH_SIZE {
        let scores = EXECUTOR_KERNEL.top_k_scores();
        result_cardinality = result_cardinality
            .saturating_add(u64::try_from(scores.len()).expect("top-k result should fit u64"));
        candidate_count = candidate_count.saturating_add(
            u64::try_from(EXECUTOR_KERNEL.top_k_candidate_count())
                .expect("top-k candidates should fit u64"),
        );
        std::hint::black_box(scores);
    }
    cassie::benchmark::KernelObservation::new(TOP_K_BATCH_SIZE, result_cardinality)
        .with_candidate_count(candidate_count)
}

pub fn tokenization_batch() -> u64 {
    let mut observed_tokens = 0_usize;
    for position in 0..TOKENIZATION_BATCH_SIZE {
        let position = usize::try_from(position).expect("tokenization batch position should fit");
        let input = TOKENIZATION_INPUTS[position % TOKENIZATION_INPUTS.len()];
        let tokens = tokenizer::tokenize(std::hint::black_box(input));
        observed_tokens = observed_tokens.saturating_add(tokens.len());
    }
    std::hint::black_box(observed_tokens);
    TOKENIZATION_BATCH_SIZE
}

pub fn bm25_score_batch() -> u64 {
    let mut accumulated_score = 0.0;
    for _ in 0..BM25_BATCH_SIZE {
        let mut score = 0.0;
        for &(term_frequency, document_frequency, document_count, document_len, average_len) in
            std::hint::black_box(&BM25_INPUTS)
        {
            score += bm25::bm25_score(
                std::hint::black_box(term_frequency),
                std::hint::black_box(document_frequency),
                std::hint::black_box(document_count),
                std::hint::black_box(1.2),
                std::hint::black_box(0.75),
                std::hint::black_box(document_len),
                std::hint::black_box(average_len),
            );
        }
        accumulated_score += std::hint::black_box(score);
    }
    std::hint::black_box(accumulated_score);
    BM25_BATCH_SIZE
}

pub fn cosine_distance_batch() -> u64 {
    let mut accumulated_distance = 0.0;
    let input_count = u64::try_from(COSINE_INPUTS.len()).expect("cosine input count should fit");
    let rounds = VECTOR_DISTANCE_BATCH_SIZE / input_count;
    for _ in 0..rounds {
        for (left, right) in std::hint::black_box(&*COSINE_INPUTS) {
            accumulated_distance += cassie::vector::cosine_distance(
                std::hint::black_box(left),
                std::hint::black_box(right),
            );
        }
    }
    std::hint::black_box(accumulated_distance);
    rounds * input_count
}

pub fn dot_product_batch() -> u64 {
    let mut accumulated_score = 0.0;
    let input_count = u64::try_from(DOT_L2_INPUTS.len()).expect("dot input count should fit");
    let rounds = VECTOR_DISTANCE_BATCH_SIZE / input_count;
    for _ in 0..rounds {
        for (left, right) in std::hint::black_box(&*DOT_L2_INPUTS) {
            accumulated_score +=
                cassie::vector::dot_score(std::hint::black_box(left), std::hint::black_box(right));
        }
    }
    std::hint::black_box(accumulated_score);
    rounds * input_count
}

pub fn l2_distance_batch() -> u64 {
    let mut accumulated_distance = 0.0;
    let input_count = u64::try_from(DOT_L2_INPUTS.len()).expect("L2 input count should fit");
    let rounds = VECTOR_DISTANCE_BATCH_SIZE / input_count;
    for _ in 0..rounds {
        for (left, right) in std::hint::black_box(&*DOT_L2_INPUTS) {
            accumulated_distance += cassie::vector::l2_distance(
                std::hint::black_box(left),
                std::hint::black_box(right),
            );
        }
    }
    std::hint::black_box(accumulated_distance);
    rounds * input_count
}

pub fn hnsw_candidate_search() -> usize {
    let candidates = (0..128)
        .map(|index| {
            let component = usize_to_f32(index) / 128.0;
            (
                format!("doc-{index}"),
                vec![component, 1.0 - component, 0.5],
            )
        })
        .collect::<Vec<_>>();
    let selected = cassie::vector::hnsw::search(
        std::hint::black_box(&[0.25, 0.75, 0.5]),
        candidates,
        10,
        cassie::vector::l2_distance,
    );
    std::hint::black_box(selected.len())
}

pub fn sql_lexing() -> usize {
    let sql = std::hint::black_box(
        "SELECT id, title FROM bench_documents WHERE score >= $1 AND status = 'approved' ORDER BY id LIMIT 20",
    );
    let mut tokens = 0usize;
    let mut in_token = false;
    for byte in sql.bytes() {
        let delimiter =
            byte.is_ascii_whitespace() || matches!(byte, b',' | b'(' | b')' | b'=' | b'<' | b'>');
        if delimiter {
            if in_token {
                tokens += 1;
                in_token = false;
            }
        } else {
            in_token = true;
        }
    }
    if in_token {
        tokens += 1;
    }
    std::hint::black_box(tokens)
}

pub fn row_to_pgwire_encoding_batch() -> u64 {
    let mut observed_bytes = 0_usize;
    for _ in 0..PGWIRE_ROW_BATCH_SIZE {
        let encoded = PGWIRE_ROW.encode();
        observed_bytes = observed_bytes.saturating_add(encoded.len());
        std::hint::black_box(encoded);
    }
    std::hint::black_box(observed_bytes);
    PGWIRE_ROW_BATCH_SIZE
}

pub fn sql_parsing() -> usize {
    let parsed =
        parse_statement("SELECT id, title FROM bench_documents WHERE score >= 10").expect("parse");
    std::hint::black_box(parsed);
    1
}
