#![allow(unused_imports, dead_code)]
use cassie::app::Cassie;
use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
use cassie::embeddings::{
    openai::OpenAiConfig, DistanceMetric, VectorIndexMetadata, VectorIndexRecord, VectorIndexType,
    DEFAULT_EMBEDDING_MODEL,
};
use cassie::midge::adapter::StorageFamily;
use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
use cntryl_midge::{TransactionMode, WriteOptions};

use super::support_sql as support;
use support::*;

include!("semantics.rs");

include!("nulls.rs");

fn float_overflow_fixture(label: &str) -> super::support_sql_fixture::SqlFixture {
    super::support_sql_fixture::sql_fixture(
        label,
        &[
            "CREATE TABLE fl (v FLOAT)",
            "INSERT INTO fl (v) VALUES (1.0e308)",
            "INSERT INTO fl (v) VALUES (1.0e308)",
        ],
    )
}

include!("errors.rs");

fn grouping_fixture(label: &str) -> super::support_sql_fixture::SqlFixture {
    super::support_sql_fixture::sql_fixture(
        label,
        &[
            "CREATE TABLE m (g TEXT, v INT)",
            "INSERT INTO m (g, v) VALUES ('a', 9)",
            "INSERT INTO m (g, v) VALUES ('a', 2)",
            "INSERT INTO m (g, v) VALUES ('b', 1)",
            "INSERT INTO m (g, v) VALUES ('c', 5)",
        ],
    )
}

fn ordered_groups_fixture(label: &str) -> super::support_sql_fixture::SqlFixture {
    super::support_sql_fixture::sql_fixture(
        label,
        &[
            "CREATE TABLE t (g TEXT, v INT)",
            "INSERT INTO t (g, v) VALUES ('a', 1)",
            "INSERT INTO t (g, v) VALUES ('a', 2)",
            "INSERT INTO t (g, v) VALUES ('b', 100)",
            "INSERT INTO t (g, v) VALUES ('c', 10)",
        ],
    )
}

fn labels(rows: &[Vec<Value>]) -> Vec<String> {
    rows.iter()
        .map(|row| match row.first() {
            Some(Value::String(label)) => label.clone(),
            _ => String::new(),
        })
        .collect()
}
