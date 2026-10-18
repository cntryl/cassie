#![allow(unused_imports, dead_code)]
use cassie::app::{Cassie, CassieSession};
use cassie::config::{
    CassieRuntimeConfig, EmbeddingsRuntimeConfig, LocalRuntimeConfig, OpenAiRuntimeConfig,
};
use cassie::embeddings::{
    openai::OpenAiConfig, DistanceMetric, VectorIndexMetadata, VectorIndexRecord, VectorIndexType,
    DEFAULT_EMBEDDING_MODEL,
};
use cassie::midge::adapter::StorageFamily;
use cassie::midge::adapter::{
    document_write_failure_point_test_guard, set_document_write_failure_point,
    DocumentWriteFailurePoint,
};
use cassie::types::{DataType, FieldSchema, Schema, Value, Vector};
use cntryl_midge::{TransactionMode, WriteOptions};

use super::support_sql as support;
use support::*;

fn setup_search_path_rollback_tables(cassie: &Cassie, session: &CassieSession) {
    cassie
        .execute_sql(session, "CREATE SCHEMA other_schema", vec![])
        .unwrap();
    cassie
        .execute_sql(
            session,
            "CREATE TABLE public.transaction_search_path (value INT)",
            vec![],
        )
        .unwrap();
    cassie
        .execute_sql(
            session,
            "CREATE TABLE other_schema.transaction_search_path (value INT)",
            vec![],
        )
        .unwrap();
    cassie
        .execute_sql(
            session,
            "INSERT INTO public.transaction_search_path VALUES (1)",
            vec![],
        )
        .unwrap();
    cassie
        .execute_sql(
            session,
            "INSERT INTO other_schema.transaction_search_path VALUES (2)",
            vec![],
        )
        .unwrap();
}

include!("transactions.rs");

include!("semantics.rs");

include!("errors.rs");
