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

include!("restart.rs");

include!("resources.rs");

fn create_indexed_and_plain_tables(
    cassie: &Cassie,
    session: &cassie::app::CassieSession,
    tables: [&str; 2],
    definition: &str,
    index_columns: &str,
    rows: &[Value],
) {
    let [indexed, plain] = tables;
    for table in tables {
        cassie
            .execute_sql(
                session,
                &format!("CREATE TABLE {table} ({definition})"),
                vec![],
            )
            .expect("create table");
    }
    cassie
        .execute_sql(
            session,
            &format!("CREATE INDEX {indexed}_idx ON {indexed} USING btree ({index_columns})"),
            vec![],
        )
        .expect("create scalar index");
    for table in [indexed, plain] {
        for value in rows {
            cassie
                .execute_sql(
                    session,
                    &format!("INSERT INTO {table} (x) VALUES ($1)"),
                    vec![value.clone()],
                )
                .expect("insert row");
        }
    }
}

include!("parameters.rs");

include!("nulls.rs");
