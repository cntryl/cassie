#![allow(dead_code, unused_imports)]

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};
use std::thread;
use std::time::{Duration, Instant};

use cassie::app::{Cassie, CassieError, CassieSession};
use cassie::catalog::{CollectionSchema, FieldMeta};
use cassie::config::{
    CassieRuntimeConfig, EmbeddingsRuntimeConfig, SelfHostedEmbeddingRuntimeConfig,
};
use cassie::pgwire::protocol::ServerMessage;
use cassie::planner::{logical, physical};
use cassie::rest::{documents, search};
use cassie::runtime::ExecutionMode;
use cassie::search::{bm25, tokenizer};
use cassie::sql::{binder, parameter_count, parameter_type_oids, parse_statement};
use cassie::types::{DataType, FieldSchema, Schema, Value};
use cntryl_lexkey::Encoder;
use serde_json::json;
use uuid::Uuid;

use super::context::{usize_to_f32, BenchContext, QueryBreakdownMicros};

static FIELD_LOOKUP_FIELDS: LazyLock<[FieldMeta; 4]> = LazyLock::new(|| {
    [
        FieldMeta {
            name: "id".to_string(),
            data_type: DataType::Text,
            is_indexed: true,
            boost: Some(1.0),
        },
        FieldMeta {
            name: "title".to_string(),
            data_type: DataType::Text,
            is_indexed: true,
            boost: Some(1.0),
        },
        FieldMeta {
            name: "body".to_string(),
            data_type: DataType::Text,
            is_indexed: true,
            boost: Some(1.0),
        },
        FieldMeta {
            name: "score".to_string(),
            data_type: DataType::Int,
            is_indexed: true,
            boost: None,
        },
    ]
});

pub fn row_encode_decode() -> usize {
    let input = std::hint::black_box(br#"{"id":"doc-1","title":"alpha","score":42}"#);
    let decoded: serde_json::Value = serde_json::from_slice(input).expect("decode row");
    let encoded = serde_json::to_vec(std::hint::black_box(&decoded)).expect("encode row");
    std::hint::black_box(encoded.len())
}

pub fn key_encode_decode() -> usize {
    let id = Uuid::new_v4().to_string();
    let prefix = lexkey_prefix(b"schema");
    let mut encoder = Encoder::with_capacity(prefix.len() + id.len());
    encoder.encode_bytes_into(&prefix);
    encoder.encode_string_into(&id);
    let key = encoder.into_vec();
    let decoded = std::str::from_utf8(key.strip_prefix(prefix.as_slice()).expect("key prefix"))
        .expect("utf8 suffix");
    let mut reencoder = Encoder::with_capacity(prefix.len() + decoded.len());
    reencoder.encode_bytes_into(&prefix);
    reencoder.encode_string_into(decoded);
    let encoded_again = reencoder.into_vec();
    std::hint::black_box(encoded_again);
    1
}

fn lexkey_prefix(family: &[u8]) -> Vec<u8> {
    let parts = [
        b"cassie".as_slice(),
        b"lexkey".as_slice(),
        b"v2".as_slice(),
        family,
    ];
    let capacity = parts.iter().map(|part| part.len()).sum::<usize>() + parts.len();
    let mut encoder = Encoder::with_capacity(capacity);
    encoder.encode_composite_into_buf(&parts);
    encoder.push_separator();
    encoder.into_vec()
}

pub fn field_lookup() -> usize {
    let schema = CollectionSchema {
        collection: "bench".to_string(),
        fields: vec![
            FieldMeta {
                name: "id".to_string(),
                data_type: DataType::Text,
                is_indexed: true,
                boost: Some(1.0),
            },
            FieldMeta {
                name: "title".to_string(),
                data_type: DataType::Text,
                is_indexed: true,
                boost: Some(1.0),
            },
        ],
    };
    std::hint::black_box(schema.field("title").expect("field"));
    1
}

pub fn field_lookup_by_field_id() -> usize {
    let mut hits = 0usize;
    for index in 0..64 {
        let field_id = std::hint::black_box(index % FIELD_LOOKUP_FIELDS.len());
        if !FIELD_LOOKUP_FIELDS[field_id].name.is_empty() {
            hits = hits.saturating_add(1);
        }
    }
    std::hint::black_box(hits)
}

pub fn predicate_evaluation() -> usize {
    let row = json!({"score": 42, "status": "approved"});
    let passes = row["score"].as_i64().unwrap_or_default() >= 40
        && row["status"].as_str() == Some("approved");
    std::hint::black_box(usize::from(passes))
}

pub fn batch_filter() -> usize {
    let scores = [
        1_i64, 10, 100, 3, 25, 8, 99, 7, 44, 61, 2, 88, 13, 55, 34, 21, 5, 89, 144, 233, 377, 610,
        987, 1_597, 4, 6, 9, 12, 18, 27, 81, 243,
    ];
    let threshold = std::hint::black_box(10_i64);
    let rows = scores
        .iter()
        .filter(|score| std::hint::black_box(**score) >= threshold)
        .count();
    std::hint::black_box(rows)
}

pub fn batch_projection() -> usize {
    let row = json!({"id":"doc-1","title":"alpha","body":"beta"});
    let projected = json!({"title": row["title"].clone()});
    std::hint::black_box(projected.as_object().map_or(0, serde_json::Map::len))
}

pub fn value_comparison() -> usize {
    let values = [
        (Value::Int64(1), Value::Int64(2)),
        (Value::Int64(3), Value::Int64(5)),
        (Value::Int64(8), Value::Int64(13)),
        (Value::Int64(21), Value::Int64(34)),
        (Value::Int64(55), Value::Int64(89)),
        (Value::Int64(144), Value::Int64(233)),
        (Value::Int64(377), Value::Int64(610)),
        (Value::Int64(987), Value::Int64(1_597)),
    ];
    let matches = values
        .iter()
        .filter(|(left, right)| {
            std::hint::black_box(left.as_i64().unwrap_or_default())
                < std::hint::black_box(right.as_i64().unwrap_or_default())
        })
        .count();
    std::hint::black_box(matches)
}

pub fn top_k_update() -> usize {
    let mut heap = BinaryHeap::new();
    for score in [3, 1, 7, 2, 9, 4] {
        heap.push(Reverse(score));
        if heap.len() > 3 {
            let _ = heap.pop();
        }
    }
    std::hint::black_box(heap.len())
}

pub fn tokenization() -> usize {
    let tokens = tokenizer::tokenize("Alpha beta, gamma and delta");
    std::hint::black_box(tokens.len())
}

pub fn bm25_score() -> usize {
    let inputs = [
        (3.0, 10.0, 1_000.0, 120.0, 100.0),
        (4.0, 11.0, 1_000.0, 118.0, 100.0),
        (5.0, 12.0, 1_000.0, 116.0, 100.0),
        (6.0, 13.0, 1_000.0, 114.0, 100.0),
        (7.0, 14.0, 1_000.0, 112.0, 100.0),
        (8.0, 15.0, 1_000.0, 110.0, 100.0),
        (9.0, 16.0, 1_000.0, 108.0, 100.0),
        (10.0, 17.0, 1_000.0, 106.0, 100.0),
    ];
    let mut total = 0.0;
    for (term_frequency, document_frequency, document_count, document_len, average_len) in inputs {
        total += bm25::bm25_score(
            std::hint::black_box(term_frequency),
            std::hint::black_box(document_frequency),
            std::hint::black_box(document_count),
            std::hint::black_box(1.2),
            std::hint::black_box(0.75),
            std::hint::black_box(document_len),
            std::hint::black_box(average_len),
        );
    }
    std::hint::black_box(total);
    inputs.len()
}

pub fn cosine_distance() -> usize {
    let mut total = 0.0;
    for index in 0..32 {
        let shift = std::hint::black_box(usize_to_f32(index) / 1_000.0);
        let left = [1.0 + shift, 0.0, 0.0, 0.5, 0.25, 0.75, 0.125, 0.875];
        let right = [0.5, 0.5 + shift, 0.0, 0.25, 0.75, 0.125, 0.875, 1.0];
        total += cassie::vector::cosine_distance(&left, &right);
    }
    std::hint::black_box(total);
    32
}

pub fn dot_product() -> usize {
    let mut total = 0.0;
    for index in 0..32 {
        let shift = std::hint::black_box(usize_to_f32(index) / 1_000.0);
        let left = [1.0 + shift, 2.0, 3.0, 5.0, 8.0, 13.0, 21.0, 34.0];
        let right = [
            0.5,
            0.5 + shift,
            0.5,
            0.25,
            0.125,
            0.0625,
            0.03125,
            0.015_625,
        ];
        total += cassie::vector::dot_score(&left, &right);
    }
    std::hint::black_box(total);
    32
}

pub fn l2_distance() -> usize {
    let mut total = 0.0;
    for index in 0..32 {
        let shift = std::hint::black_box(usize_to_f32(index) / 1_000.0);
        let left = [1.0 + shift, 2.0, 3.0, 5.0, 8.0, 13.0, 21.0, 34.0];
        let right = [
            0.5,
            0.5 + shift,
            0.5,
            0.25,
            0.125,
            0.0625,
            0.03125,
            0.015_625,
        ];
        total += cassie::vector::l2_distance(&left, &right);
    }
    std::hint::black_box(total);
    32
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

pub fn parameter_binding() -> usize {
    let parsed =
        parse_statement("SELECT * FROM bench WHERE id = $1 AND score = $2").expect("parse");
    let count = parameter_count(&parsed);
    let types = parameter_type_oids(&parsed, &[25, 23]);
    std::hint::black_box(types);
    count
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
    let message = ServerMessage::DataRow(vec!["alpha".to_string(), "1".to_string()]);
    let encoded = cassie::pgwire::protocol::encode(&message);
    std::hint::black_box(encoded.len())
}

pub fn row_to_json_encoding() -> usize {
    let row = json!({"id":"doc-1","title":"alpha","score":1});
    let encoded = serde_json::to_vec(&row).expect("json encode");
    std::hint::black_box(encoded.len())
}

pub fn sql_parsing() -> usize {
    let parsed =
        parse_statement("SELECT id, title FROM bench_documents WHERE score >= 10").expect("parse");
    std::hint::black_box(parsed);
    1
}
