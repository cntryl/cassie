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

type VectorPair = ([f32; 8], [f32; 8]);
type Bm25Input = (f64, f64, f64, f64, f64);

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
            [1.0 + shift, 0.0, 0.0, 0.5, 0.25, 0.75, 0.125, 0.875],
            [0.5, 0.5 + shift, 0.0, 0.25, 0.75, 0.125, 0.875, 1.0],
        )
    })
});
static DOT_L2_INPUTS: LazyLock<[VectorPair; 32]> = LazyLock::new(|| {
    std::array::from_fn(|index| {
        let shift = usize_to_f32(index) / 1_000.0;
        (
            [1.0 + shift, 2.0, 3.0, 5.0, 8.0, 13.0, 21.0, 34.0],
            [
                0.5,
                0.5 + shift,
                0.5,
                0.25,
                0.125,
                0.0625,
                0.03125,
                0.015_625,
            ],
        )
    })
});

pub fn prepare_hotpath(workload: &str) -> Result<(), &'static str> {
    match workload {
        "row_encode_decode" => {
            LazyLock::force(&ROW_CODEC_KERNEL);
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
        "tokenization" | "bm25_scoring" => {}
        _ => return Err("unknown Tier 1 hot-path workload"),
    }
    Ok(())
}

pub fn row_encode_decode() -> usize {
    let (encoded, decoded) = ROW_CODEC_KERNEL.round_trip();
    let encoded_len = encoded.len();
    std::hint::black_box((encoded, decoded));
    encoded_len
}

pub fn key_encode_decode() -> usize {
    let (encoded, decoded) = ROW_KEY_KERNEL.encode_decode();
    let encoded_len = encoded.len();
    std::hint::black_box((encoded, decoded));
    encoded_len
}

pub fn predicate_evaluation() -> usize {
    std::hint::black_box(usize::from(EXECUTOR_KERNEL.predicate_matches()))
}

pub fn batch_filter() -> usize {
    std::hint::black_box(EXECUTOR_KERNEL.filter_batch())
}

pub fn batch_projection() -> usize {
    std::hint::black_box(EXECUTOR_KERNEL.project_row().len())
}

pub fn value_comparison() -> usize {
    std::hint::black_box(usize::from(EXECUTOR_KERNEL.value_comparison_matches()))
}

pub fn top_k_update() -> cassie::benchmark::KernelObservation {
    let scores = EXECUTOR_KERNEL.top_k_scores();
    let result_cardinality = u64::try_from(scores.len()).expect("top-k result should fit u64");
    let candidate_count = u64::try_from(EXECUTOR_KERNEL.top_k_candidate_count())
        .expect("top-k candidates should fit u64");
    std::hint::black_box(scores);
    cassie::benchmark::KernelObservation::new(1, result_cardinality)
        .with_candidate_count(candidate_count)
}

pub fn tokenization() -> usize {
    let tokens = tokenizer::tokenize("Alpha beta, gamma and delta");
    std::hint::black_box(tokens.len())
}

pub fn bm25_score() -> usize {
    let (term_frequency, document_frequency, document_count, document_len, average_len) =
        BM25_INPUTS[0];
    let score = bm25::bm25_score(
        std::hint::black_box(term_frequency),
        std::hint::black_box(document_frequency),
        std::hint::black_box(document_count),
        std::hint::black_box(1.2),
        std::hint::black_box(0.75),
        std::hint::black_box(document_len),
        std::hint::black_box(average_len),
    );
    std::hint::black_box(score);
    1
}

pub fn cosine_distance() -> usize {
    let (left, right) = &COSINE_INPUTS[0];
    let distance =
        cassie::vector::cosine_distance(std::hint::black_box(left), std::hint::black_box(right));
    std::hint::black_box(distance);
    1
}

pub fn dot_product() -> usize {
    let (left, right) = &DOT_L2_INPUTS[0];
    let score = cassie::vector::dot_score(std::hint::black_box(left), std::hint::black_box(right));
    std::hint::black_box(score);
    1
}

pub fn l2_distance() -> usize {
    let (left, right) = &DOT_L2_INPUTS[0];
    let distance =
        cassie::vector::l2_distance(std::hint::black_box(left), std::hint::black_box(right));
    std::hint::black_box(distance);
    1
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

pub fn row_to_pgwire_encoding() -> usize {
    let encoded = PGWIRE_ROW.encode();
    std::hint::black_box(encoded.len())
}

pub fn sql_parsing() -> usize {
    let parsed =
        parse_statement("SELECT id, title FROM bench_documents WHERE score >= 10").expect("parse");
    std::hint::black_box(parsed);
    1
}
