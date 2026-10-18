#![allow(unused_imports, dead_code)]

use super::support_pgwire as pgwire_support;

use cassie::app::{Cassie, CassieSession};
use cassie::catalog::{IndexKind, IndexMeta};
use cassie::midge::adapter::set_operator_feedback_persistence_failure_point;
use cassie::midge::StorageFamily;
use cassie::runtime::{RuntimeFeedbackKey, RuntimeFeedbackObservation, RuntimeFeedbackRecord};
use cassie::types::{DataType, FieldSchema, Schema};
use pgwire_support::{data_dir, describe_statement_frame, startup_frame, use_local_storage};
use std::collections::BTreeMap;
use std::time::Duration;

fn operator_feedback_config(enabled: bool) -> cassie::config::CassieRuntimeConfig {
    let mut config = cassie::config::CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.operator_feedback_enabled = enabled;
    config
}

fn feedback_key(
    cassie: &Cassie,
    session: &CassieSession,
    sql: &str,
    candidate_index: Option<&str>,
) -> RuntimeFeedbackKey {
    cassie
        .read_operator_feedback_key_for_diagnostics(session, sql, candidate_index)
        .expect("feedback key")
}

fn register_feedback_collection(cassie: &Cassie, collection: &str) {
    let schema = Schema {
        fields: vec![
            FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
            FieldSchema {
                name: "body".to_string(),
                data_type: DataType::Text,
                nullable: true,
            },
        ],
    };
    cassie
        .midge
        .create_collection(collection, schema.clone())
        .unwrap();
    cassie.register_collection(collection, schema);
    cassie
        .midge
        .put_document(
            collection,
            Some("doc-1".to_string()),
            serde_json::json!({"title": "alpha", "body": "one"}),
        )
        .unwrap();
    cassie
        .midge
        .put_document(
            collection,
            Some("doc-2".to_string()),
            serde_json::json!({"title": "beta", "body": "two"}),
        )
        .unwrap();
}

fn adaptive_candidate_config(min: usize, max: usize) -> cassie::config::CassieRuntimeConfig {
    let mut config = cassie::config::CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.adaptive_candidate_min = min;
    config.limits.adaptive_candidate_max = max;
    config
}

fn register_operator_feedback_indexes(
    cassie: &Cassie,
    collection: &str,
    first_index: &str,
    second_index: &str,
) {
    for (field, index_name) in [("body", first_index), ("title", second_index)] {
        let index = IndexMeta {
            collection: collection.to_string(),
            name: index_name.to_string(),
            field: field.to_string(),
            fields: vec![field.to_string()],
            expressions: Vec::new(),
            include_fields: Vec::new(),
            predicate: None,
            kind: IndexKind::Scalar,
            unique: false,
            options: BTreeMap::default(),
        };
        cassie.midge.put_index(&index).unwrap();
        cassie.catalog.register_index(index);
    }
}

fn confident_feedback(elapsed_ms: u64, storage_reads: u64) -> RuntimeFeedbackObservation {
    RuntimeFeedbackObservation {
        rows_in: storage_reads.max(1),
        rows_out: 1,
        elapsed_ms,
        storage_reads,
        ..RuntimeFeedbackObservation::default()
    }
}

fn register_adaptive_candidate_collection(cassie: &Cassie, collection: &str) {
    let schema = Schema {
        fields: vec![FieldSchema {
            name: "body".to_string(),
            data_type: DataType::Text,
            nullable: true,
        }],
    };
    cassie
        .midge
        .create_collection(collection, schema.clone())
        .unwrap();
    cassie.register_collection(collection, schema);
    for (id, body) in [
        ("doc-1", "alpha shared"),
        ("doc-2", "alpha shared"),
        ("doc-3", "alpha shared"),
    ] {
        cassie
            .midge
            .put_document(
                collection,
                Some(id.to_string()),
                serde_json::json!({"body": body}),
            )
            .unwrap();
    }
}

async fn read_auth_frame(
    reader: &mut tokio::io::BufReader<tokio::net::tcp::ReadHalf<'_>>,
) -> (u8, i32, Vec<u8>) {
    let mut header = [0u8; 5];
    tokio::io::AsyncReadExt::read_exact(reader, &mut header)
        .await
        .expect("read auth frame header");

    let tag = header[0];
    let len = i32::from_be_bytes(header[1..].try_into().expect("auth frame length"));
    let mut payload =
        vec![0u8; usize::try_from(len - 4).expect("non-negative auth payload length")];
    tokio::io::AsyncReadExt::read_exact(reader, &mut payload)
        .await
        .expect("read auth frame payload");

    (tag, len, payload)
}

async fn read_wire_frame(
    reader: &mut tokio::io::BufReader<tokio::net::tcp::ReadHalf<'_>>,
) -> (u8, Vec<u8>) {
    pgwire_support::read_wire_frame(reader).await
}

include!("semantics.rs");

include!("parameters.rs");

include!("restart.rs");

include!("errors.rs");

include!("resources.rs");
