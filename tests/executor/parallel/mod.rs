#![allow(unused_imports, dead_code)]
use cassie::app::Cassie;
use cassie::catalog::{IndexKind, IndexMeta};
use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
use cassie::embeddings::{openai::OpenAiConfig, DistanceMetric, DEFAULT_EMBEDDING_MODEL};
use cassie::executor;
use cassie::planner::logical::LogicalPlan;
use cassie::planner::physical::PhysicalPlan;
use cassie::sql::ast::{Expr, FunctionCall, QuerySource, SelectItem};
use cassie::sql::binder;
use cassie::sql::parser;
use cassie::types::{DataType, FieldSchema, Schema, Value};
use std::collections::BTreeMap;
use uuid::Uuid;

use super::support_executor as support;
use support::*;

const PARALLEL_ROW_COUNT: usize = 1025;

fn parallel_row_count_i64() -> i64 {
    i64::try_from(PARALLEL_ROW_COUNT).expect("parallel row count should fit i64")
}

fn usize_to_i64(value: usize) -> i64 {
    i64::try_from(value).expect("test row count should fit i64")
}

fn create_registered_collection(cassie: &Cassie, collection: &str, fields: &[(&str, DataType)]) {
    let schema = Schema {
        fields: fields
            .iter()
            .map(|(name, data_type)| FieldSchema {
                name: (*name).to_string(),
                data_type: data_type.clone(),
                nullable: true,
            })
            .collect(),
    };
    cassie
        .midge
        .create_collection(collection, schema.clone())
        .unwrap();
    cassie.register_collection(
        collection,
        schema
            .fields
            .iter()
            .map(|field| (field.name.clone(), field.data_type.clone()))
            .collect(),
    );
}

fn put_documents(
    cassie: &Cassie,
    collection: &str,
    documents: impl IntoIterator<Item = (String, serde_json::Value)>,
) {
    cassie
        .midge
        .put_documents(
            collection,
            documents
                .into_iter()
                .map(|(id, payload)| (Some(id), payload))
                .collect(),
        )
        .unwrap();
}

#[path = "aggregates/boolean/mod.rs"]
mod postgresql18_boolean_aggregates;
#[path = "aggregates/collections.rs"]
mod postgresql18_collection_aggregate_wave3;

include!("semantics.rs");

include!("resources.rs");

include!("errors.rs");

/// Runs `sql` over `values` (one row per value, stored in scan order) with
/// the given aggregation worker limit and returns the rows or the error text.
fn aggregate_with_workers(
    label: &str,
    workers: usize,
    data_type: DataType,
    values: &[serde_json::Value],
    sql: &str,
) -> Result<Vec<Vec<Value>>, String> {
    let path = data_dir(label);
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.parallel_aggregation_workers = workers;
    let cassie = Cassie::new_in_memory_with_config(config).unwrap();
    let session = cassie.create_session("tester", None);
    create_registered_collection(&cassie, "exec_parallel_row_order", &[("v", data_type)]);
    put_documents(
        &cassie,
        "exec_parallel_row_order",
        values
            .iter()
            .enumerate()
            .map(|(index, value)| (format!("row-{index:07}"), serde_json::json!({ "v": value }))),
    );
    let result = cassie
        .execute_sql(&session, sql, vec![])
        .map(|result| result.rows().to_vec())
        .map_err(|error| error.to_string());
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
    result
}
