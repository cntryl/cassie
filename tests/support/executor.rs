#![allow(unused_imports, dead_code)]
use cassie::app::Cassie;
use cassie::catalog::{IndexKind, IndexMeta};
use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
use cassie::embeddings::{openai::OpenAiConfig, DEFAULT_EMBEDDING_MODEL};
use cassie::types::{DataType, FieldSchema, Schema};
use serde_json::Value;
use std::collections::BTreeMap;
use std::env;
use uuid::Uuid;

pub fn with_fallback() {
    env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
    env::set_var("CASSIE_MIDGE_DATA_DIR", data_dir("fallback"));
}

pub fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!("cassie-exec-{}-{}", label, Uuid::new_v4()));
    path.to_string_lossy().to_string()
}

pub fn cassie_temp(label: &str) -> Cassie {
    with_fallback();
    Cassie::new_with_data_dir(data_dir(label)).expect("cassie")
}

pub fn text_schema(fields: &[&str]) -> Schema {
    Schema {
        fields: fields
            .iter()
            .map(|name| FieldSchema {
                name: (*name).to_string(),
                data_type: DataType::Text,
                nullable: true,
            })
            .collect(),
    }
}

pub fn create_text_collection(cassie: &Cassie, collection: &str, fields: &[&str]) {
    let schema = text_schema(fields);
    cassie
        .midge
        .create_collection(collection, schema.clone())
        .expect("create collection");
    cassie.register_collection(
        collection,
        schema
            .fields
            .iter()
            .map(|field| (field.name.clone(), field.data_type.clone()))
            .collect(),
    );
}

pub fn put_document(cassie: &Cassie, collection: &str, id: &str, payload: Value) {
    cassie
        .midge
        .put_document(collection, Some(id.to_string()), payload)
        .expect("put document");
}

pub fn fulltext_index(
    collection: &str,
    name: &str,
    field: &str,
    options: &[(&str, &str)],
) -> IndexMeta {
    IndexMeta {
        collection: collection.to_string(),
        name: name.to_string(),
        field: field.to_string(),
        fields: vec![field.to_string()],
        expressions: Vec::new(),
        include_fields: Vec::new(),
        predicate: None,
        kind: IndexKind::FullText,
        unique: false,
        options: options
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect::<BTreeMap<_, _>>(),
    }
}

pub fn put_fulltext_index(
    cassie: &Cassie,
    collection: &str,
    name: &str,
    field: &str,
    options: &[(&str, &str)],
) {
    cassie
        .midge
        .put_index(&fulltext_index(collection, name, field, options))
        .expect("put index");
}

pub fn openai_runtime_for_vectors() -> CassieRuntimeConfig {
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.embeddings = EmbeddingsRuntimeConfig::OpenAI(OpenAiRuntimeConfig {
        config: OpenAiConfig {
            api_key: "vector-tests".to_string(),
            model: DEFAULT_EMBEDDING_MODEL.to_string(),
        },
        timeout_seconds: 1,
        max_batch_size: 1,
        max_retries: 1,
        base_url: Some("http://127.0.0.1:1".to_string()),
    });
    config
}
