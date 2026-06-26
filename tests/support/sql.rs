#![allow(unused_imports, dead_code)]
use cassie::app::Cassie;
use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
use cassie::embeddings::NormalizedVectorRecord;
use cassie::embeddings::{openai::OpenAiConfig, DEFAULT_EMBEDDING_MODEL};
use cassie::midge::adapter::StorageFamily;
use cntryl_midge::{TransactionMode, WriteOptions};
use uuid::Uuid;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct TimeSeriesSidecarRecord {
    pub collection: String,
    pub index_name: String,
    pub id: String,
    pub bucket_key: String,
    pub timestamp: String,
}

pub fn with_fallback() {
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
    std::env::set_var("CASSIE_MIDGE_DATA_DIR", data_dir("fallback"));
}

pub fn data_dir(label: &str) -> String {
    let mut dir = std::env::temp_dir();
    dir.push(format!("cassie-sql-{}-{}", label, Uuid::new_v4()));
    dir.to_string_lossy().to_string()
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

pub fn put_legacy_document(
    cassie: &Cassie,
    collection: &str,
    id: &str,
    payload: serde_json::Value,
) {
    let mut tx = cassie.midge.data_tx(TransactionMode::ReadWrite).unwrap();
    tx.put(
        format!("doc:{collection}:{id}").into_bytes(),
        payload.to_string().into_bytes(),
        None,
    )
    .unwrap();
    tx.commit(WriteOptions::sync()).unwrap();
}

pub fn clear_normalized_sidecars(cassie: &Cassie, collection: &str, field: &str) {
    let entries = cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, b"")
        .unwrap();
    let mut tx = cassie.midge.data_tx(TransactionMode::ReadWrite).unwrap();
    for (key, value) in entries {
        let Ok(record) = serde_json::from_slice::<NormalizedVectorRecord>(&value) else {
            continue;
        };
        if record.collection == collection && record.field == field {
            tx.delete(key).unwrap();
        }
    }
    tx.commit(WriteOptions::sync()).unwrap();
}

pub fn time_series_sidecar_records(
    cassie: &Cassie,
    collection: &str,
    index_name: &str,
) -> Vec<TimeSeriesSidecarRecord> {
    cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, b"")
        .unwrap()
        .into_iter()
        .filter_map(|(_key, value)| serde_json::from_slice::<TimeSeriesSidecarRecord>(&value).ok())
        .filter(|record| record.collection == collection && record.index_name == index_name)
        .collect()
}

pub fn clear_time_series_sidecars(cassie: &Cassie, collection: &str, index_name: &str) {
    let entries = cassie
        .midge
        .raw_scan_prefix(StorageFamily::Data, b"")
        .unwrap();
    let mut tx = cassie.midge.data_tx(TransactionMode::ReadWrite).unwrap();
    for (key, value) in entries {
        let Ok(record) = serde_json::from_slice::<TimeSeriesSidecarRecord>(&value) else {
            continue;
        };
        if record.collection == collection && record.index_name == index_name {
            tx.delete(key).unwrap();
        }
    }
    tx.commit(WriteOptions::sync()).unwrap();
}

pub fn explain_plan_text(result: &cassie::executor::QueryResult) -> &str {
    match &result.rows[0][0] {
        cassie::types::Value::String(plan) => plan,
        _ => panic!("expected textual plan"),
    }
}

pub fn assert_explain_contains(plan: &str, key: &str, value: &str) {
    let needle = format!("{key}={value}");
    assert!(
        plan.contains(&needle),
        "expected '{needle}' in plan: {plan}"
    );
}
