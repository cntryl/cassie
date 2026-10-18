#![allow(unused_imports, dead_code)]

use super::support_adaptive_evidence::AdaptiveProfileEvidence;
use super::support_pgwire as pgwire_support;

use cassie::app::{Cassie, CassieSession};
use cassie::catalog::{IndexKind, IndexMeta};
use cassie::config::OperatorSwitchingEnabled;
use cassie::runtime::{QueryCancellationHandle, RuntimeFeedbackKey, RuntimeFeedbackObservation};
use cassie::sql::parser;
use cassie::types::{DataType, FieldSchema, Schema};
use pgwire_support::{data_dir, describe_statement_frame, startup_frame, use_local_storage};

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

fn adaptive_execution_config(
    enabled: bool,
    min_cost_savings_bps: usize,
) -> cassie::config::CassieRuntimeConfig {
    let mut config = cassie::config::CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.operator_feedback_enabled = true;
    config.limits.adaptive_execution_enabled = enabled;
    config.limits.adaptive_min_cost_savings_bps = min_cost_savings_bps;
    config
}

fn adaptive_execution_confidence_config(
    enabled: bool,
    min_cost_savings_bps: usize,
    min_confidence_bps: u16,
) -> cassie::config::CassieRuntimeConfig {
    let mut config = adaptive_execution_config(enabled, min_cost_savings_bps);
    config.limits.adaptive_min_confidence_bps = min_confidence_bps;
    config
}

fn snapshot_delta(after: &serde_json::Value, before: &serde_json::Value, path: &[&str]) -> u64 {
    let after_value = path.iter().fold(after, |value, key| &value[*key]);
    let before_value = path.iter().fold(before, |value, key| &value[*key]);
    after_value.as_u64().unwrap_or_default() - before_value.as_u64().unwrap_or_default()
}

fn operator_switch_config(enabled: bool, threshold: usize) -> cassie::config::CassieRuntimeConfig {
    let mut config = cassie::config::CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.vectorized_joins_enabled = true;
    config.limits.vectorized_join_batch_size = 2;
    config.limits.operator_switching_enabled = if enabled {
        OperatorSwitchingEnabled::enabled()
    } else {
        OperatorSwitchingEnabled::disabled()
    };
    config.limits.operator_switch_join_row_threshold = threshold;
    config
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
            options: std::collections::BTreeMap::default(),
        };
        cassie.midge.put_index(&index).unwrap();
        cassie.catalog.register_index(index);
    }
}

fn create_switch_join_tables(cassie: &Cassie, session: &CassieSession, users: &str, orders: &str) {
    cassie
        .execute_sql(
            session,
            &format!("CREATE TABLE {users} (user_key INT, name TEXT)"),
            vec![],
        )
        .unwrap();
    cassie
        .execute_sql(
            session,
            &format!("CREATE TABLE {orders} (order_user_key INT, total INT)"),
            vec![],
        )
        .unwrap();
    cassie
        .execute_sql(
            session,
            &format!("INSERT INTO {users} (user_key, name) VALUES (1, 'ada'), (2, 'grace')"),
            vec![],
        )
        .unwrap();
    cassie
        .execute_sql(
            session,
            &format!("INSERT INTO {orders} (order_user_key, total) VALUES (1, 10), (2, 20)"),
            vec![],
        )
        .unwrap();
}

fn create_persisted_adaptive_fixture(
    cassie: &Cassie,
    session: &CassieSession,
    collection: &str,
    base_index: &str,
    preferred_index: &str,
) {
    cassie
        .execute_sql(
            session,
            &format!("CREATE TABLE {collection} (title TEXT, body TEXT)"),
            vec![],
        )
        .unwrap();
    let rows = std::iter::repeat_n("('alpha', 'one')", 8)
        .chain(std::iter::once("('beta', 'two')"))
        .collect::<Vec<_>>()
        .join(", ");
    cassie
        .execute_sql(
            session,
            &format!("INSERT INTO {collection} (title, body) VALUES {rows}"),
            vec![],
        )
        .unwrap();
    for (index, field) in [(base_index, "body"), (preferred_index, "title")] {
        cassie
            .execute_sql(
                session,
                &format!("CREATE INDEX {index} ON {collection} USING btree ({field})"),
                vec![],
            )
            .unwrap();
    }
}

fn adaptive_candidate_config(min: usize, max: usize) -> cassie::config::CassieRuntimeConfig {
    let mut config = cassie::config::CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.adaptive_candidate_min = min;
    config.limits.adaptive_candidate_max = max;
    config
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

include!("errors.rs");

include!("resources.rs");

include!("restart.rs");

include!("cancellation.rs");
