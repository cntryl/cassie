#![allow(unused_imports, dead_code)]
use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, OpenAiRuntimeConfig};
use cassie::embeddings::{openai::OpenAiConfig, DEFAULT_EMBEDDING_MODEL};
use std::env;
use uuid::Uuid;

pub fn with_fallback() {
    env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
}

pub fn data_dir(label: &str) -> String {
    let mut path = std::env::temp_dir();
    path.push(format!("cassie-exec-{}-{}", label, Uuid::new_v4()));
    path.to_string_lossy().to_string()
}

pub fn openai_runtime_for_vectors() -> CassieRuntimeConfig {
    let mut config = CassieRuntimeConfig::from_env();
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
