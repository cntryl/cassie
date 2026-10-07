use cassie::app::Cassie;
use cassie::config::{
    CassieRuntimeConfig, EmbeddingsRuntimeConfig, ExecutionResultCacheEnabled, LocalRuntimeConfig,
};
use cassie::embeddings::{Embedding, EmbeddingError, EmbeddingProvider};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Debug, Default)]
pub struct Provider(pub AtomicUsize);
impl EmbeddingProvider for Provider {
    fn provider_name(&self) -> &'static str {
        "local"
    }
    fn model_name(&self) -> &'static str {
        "vector-qualification"
    }
    fn dimensions(&self) -> usize {
        3
    }
    fn embed_documents(&self, texts: &[String]) -> Result<Vec<Embedding>, EmbeddingError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        texts
            .iter()
            .map(|text| match text.as_str() {
                "zero" => Ok(Embedding {
                    values: vec![0.0; 3],
                }),
                "unit" => Ok(Embedding {
                    values: vec![1.0, 0.0, 0.0],
                }),
                _ => Err(EmbeddingError::InvalidConfiguration(
                    "unavailable qualification query".into(),
                )),
            })
            .collect()
    }
}
pub const ROWS: [(&str, [f32; 3]); 5] = [
    ("A", [0.0; 3]),
    ("a", [0.0; 3]),
    ("positive", [1.0, 0.0, 0.0]),
    ("negative", [-1.0, 0.0, 0.0]),
    ("orthogonal", [0.0, 1.0, 0.0]),
];
pub fn fixture(metric: &str, options: Option<&str>) -> (Cassie, String, Arc<Provider>) {
    let path = std::env::temp_dir()
        .join(format!(
            "cassie-vector-qualification-{}",
            uuid::Uuid::new_v4()
        ))
        .to_string_lossy()
        .into_owned();
    let mut config = CassieRuntimeConfig::from_env().expect("config");
    config.embeddings = EmbeddingsRuntimeConfig::Local(LocalRuntimeConfig {
        model: "vector-qualification".into(),
        dimensions: 3,
    });
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    let mut cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("Cassie");
    let provider = Arc::new(Provider::default());
    cassie.embedding_provider = provider.clone();
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE vectors (id TEXT, content TEXT, embedding VECTOR(3))",
            vec![],
        )
        .expect("table");
    if let Some(options) = options {
        cassie.execute_sql(&session, &format!("CREATE INDEX qualified_vector ON vectors USING vector(embedding) WITH (source_field = content, metric = {metric}, {options})"), vec![]).expect("index");
    }
    let mut documents: Vec<_> = ROWS
        .iter()
        .map(|(id, vector)| {
            (
                Some((*id).into()),
                serde_json::json!({"id": format!("declared-{id}"), "embedding": vector}),
            )
        })
        .collect();
    documents.push((Some("missing".into()), serde_json::json!({"id": "missing"})));
    documents.push((
        Some("null".into()),
        serde_json::json!({"id": "null", "embedding": null}),
    ));
    cassie
        .midge
        .put_documents("vectors", documents)
        .expect("seed");
    let collection = cassie
        .catalog
        .get_schema("vectors")
        .expect("schema")
        .collection;
    let stats = cassie
        .midge
        .rebuild_cardinality_stats_for_collection(&collection)
        .expect("authoritative fixture cardinality");
    cassie.catalog.set_cardinality_stats(&collection, stats);
    (cassie, path, provider)
}
pub fn oracle(metric: &str, query: &[f32], descending: bool) -> Vec<(String, f64)> {
    let mut rows: Vec<(String, f64)> = ROWS
        .iter()
        .map(|(id, vector)| {
            let dot: f64 = vector
                .iter()
                .zip(query)
                .map(|(a, b)| f64::from(*a) * f64::from(*b))
                .sum();
            let norm_a: f64 = vector.iter().map(|a| f64::from(*a).powi(2)).sum();
            let norm_b: f64 = query.iter().map(|a| f64::from(*a).powi(2)).sum();
            let distance = match metric {
                "l2" => vector
                    .iter()
                    .zip(query)
                    .map(|(a, b)| (f64::from(*a) - f64::from(*b)).powi(2))
                    .sum::<f64>()
                    .sqrt(),
                "dot" => -dot,
                "cosine" if norm_a == 0.0 || norm_b == 0.0 => 1.0,
                "cosine" => 1.0 - dot / (norm_a.sqrt() * norm_b.sqrt()),
                _ => panic!("unknown metric"),
            };
            ((*id).into(), distance)
        })
        .collect();
    rows.sort_by(|a, b| {
        let order = a.1.total_cmp(&b.1);
        (if descending { order.reverse() } else { order }).then(a.0.cmp(&b.0))
    });
    rows
}

pub fn reopen(path: &str) -> Cassie {
    let mut config = CassieRuntimeConfig::from_env().expect("config");
    config.embeddings = EmbeddingsRuntimeConfig::Local(LocalRuntimeConfig {
        model: "vector-qualification".into(),
        dimensions: 3,
    });
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    let mut cassie = Cassie::new_with_data_dir_and_config(path, config).expect("reopen Cassie");
    cassie.embedding_provider = Arc::new(Provider::default());
    cassie.startup().expect("startup persisted fixture");
    cassie
}

pub fn remove_one_normalized_sidecar(cassie: &Cassie, collection: &str) {
    let prefix = cassie
        .midge
        .normalized_vector_prefix_for_diagnostics(collection, "embedding")
        .expect("prefix");
    let records = cassie
        .midge
        .raw_scan_prefix(cassie::midge::adapter::StorageFamily::Data, &prefix)
        .expect("sidecars");
    let mut transaction = cassie
        .midge
        .data_tx(cntryl_midge::TransactionMode::ReadWrite)
        .expect("transaction");
    transaction
        .delete(records.first().expect("nonempty sidecars").0.clone())
        .expect("delete one sidecar");
    transaction
        .commit(cntryl_midge::WriteOptions::sync())
        .expect("commit");
}

pub fn keep_one_normalized_sidecar(cassie: &Cassie, collection: &str) {
    let prefix = cassie
        .midge
        .normalized_vector_prefix_for_diagnostics(collection, "embedding")
        .expect("prefix");
    let records = cassie
        .midge
        .raw_scan_prefix(cassie::midge::adapter::StorageFamily::Data, &prefix)
        .expect("sidecars");
    assert_eq!(records.len(), 5);
    let mut transaction = cassie
        .midge
        .data_tx(cntryl_midge::TransactionMode::ReadWrite)
        .expect("transaction");
    for (key, _) in records.into_iter().skip(1) {
        transaction.delete(key).expect("delete partial sidecar");
    }
    transaction
        .commit(cntryl_midge::WriteOptions::sync())
        .expect("commit");
}
