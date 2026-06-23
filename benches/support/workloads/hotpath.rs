#![allow(dead_code, unused_imports)]

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
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

use super::context::{BenchContext, QueryBreakdownMicros};

pub fn row_encode_decode() -> usize {
    let encoded = serde_json::to_vec(&json!({"id":"doc-1","title":"alpha"})).expect("encode row");
    let decoded: serde_json::Value = serde_json::from_slice(&encoded).expect("decode row");
    std::hint::black_box(decoded);
    1
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
    let reencoded = reencoder.into_vec();
    std::hint::black_box(reencoded);
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
    let fields = [
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
    ];
    let field_id = std::hint::black_box(2usize);
    std::hint::black_box(&fields[field_id]);
    1
}

pub fn predicate_evaluation() -> usize {
    let row = json!({"score": 42, "status": "approved"});
    let passes = row["score"].as_i64().unwrap_or_default() >= 40
        && row["status"].as_str() == Some("approved");
    std::hint::black_box(passes as usize)
}

pub fn batch_filter() -> usize {
    let scores = [1_i64, 10, 100, 3, 25, 8, 99, 7];
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
    std::hint::black_box(
        projected
            .as_object()
            .map(|fields| fields.len())
            .unwrap_or(0),
    )
}

pub fn value_comparison() -> usize {
    let left = std::hint::black_box(Value::Int64(1));
    let right = std::hint::black_box(Value::Int64(2));
    std::hint::black_box(left.as_i64().unwrap_or_default() < right.as_i64().unwrap_or_default());
    1
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
    let score = bm25::bm25_score(
        std::hint::black_box(3.0),
        std::hint::black_box(10.0),
        std::hint::black_box(1000.0),
        std::hint::black_box(1.2),
        std::hint::black_box(0.75),
        std::hint::black_box(120.0),
        std::hint::black_box(100.0),
    );
    std::hint::black_box(score);
    1
}

pub fn cosine_distance() -> usize {
    let distance = cassie::vector::cosine_distance(&[1.0, 0.0, 0.0], &[0.5, 0.5, 0.0]);
    std::hint::black_box(distance);
    1
}

pub fn dot_product() -> usize {
    let score = cassie::vector::dot_score(&[1.0, 2.0, 3.0], &[0.5, 0.5, 0.5]);
    std::hint::black_box(score);
    1
}

pub fn l2_distance() -> usize {
    let distance = cassie::vector::l2_distance(&[1.0, 2.0, 3.0], &[0.5, 0.5, 0.5]);
    std::hint::black_box(distance);
    1
}

pub fn hnsw_candidate_search() -> usize {
    let candidates = (0..128)
        .map(|index| {
            (
                format!("doc-{index}"),
                vec![index as f32 / 128.0, 1.0 - (index as f32 / 128.0), 0.5],
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
