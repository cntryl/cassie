use std::env;

use crate::embeddings::openai::OpenAiConfig;

#[derive(Debug, Clone)]
pub struct CassieRuntimeConfig {
    pub pgwire_listen: String,
    pub rest_listen: String,
    pub user: String,
    pub password: String,
    pub embeddings: EmbeddingsRuntimeConfig,
}

#[derive(Debug, Clone)]
pub struct OpenAiRuntimeConfig {
    pub config: OpenAiConfig,
    pub timeout_seconds: u64,
    pub max_batch_size: usize,
    pub max_retries: usize,
    pub base_url: Option<String>,
}

#[derive(Debug, Clone)]
pub enum EmbeddingsRuntimeConfig {
    Disabled,
    OpenAI(OpenAiRuntimeConfig),
    Voyage,
    Cohere,
    Local,
}

impl Default for CassieRuntimeConfig {
    fn default() -> Self {
        Self {
            pgwire_listen: "127.0.0.1:5432".to_string(),
            rest_listen: "127.0.0.1:8080".to_string(),
            user: "postgres".to_string(),
            password: "postgres".to_string(),
            embeddings: EmbeddingsRuntimeConfig::Disabled,
        }
    }
}

impl Default for OpenAiRuntimeConfig {
    fn default() -> Self {
        Self {
            config: OpenAiConfig {
                api_key: String::new(),
                model: crate::embeddings::DEFAULT_EMBEDDING_MODEL.to_string(),
            },
            timeout_seconds: 30,
            max_batch_size: 16,
            max_retries: 3,
            base_url: None,
        }
    }
}

impl CassieRuntimeConfig {
    pub fn from_env() -> Self {
        let mut config = Self::default();
        if let Ok(v) = env::var("CASSIE_PGWIRE_LISTEN") {
            config.pgwire_listen = v;
        }
        if let Ok(v) = env::var("CASSIE_REST_LISTEN") {
            config.rest_listen = v;
        }
        if let Ok(v) = env::var("CASSIE_PGWIRE_USER") {
            config.user = v;
        }
        if let Ok(v) = env::var("CASSIE_PGWIRE_PASSWORD") {
            config.password = v;
        }

        let provider = env::var("EMBEDDINGS_PROVIDER").unwrap_or_else(|_| "disabled".to_string());
        config.embeddings = parse_provider_config(provider.to_lowercase().as_str());

        config
    }
}

fn parse_provider_config(provider: &str) -> EmbeddingsRuntimeConfig {
    match provider {
        "openai" => EmbeddingsRuntimeConfig::OpenAI(OpenAiRuntimeConfig {
            config: OpenAiConfig {
                api_key: env::var("OPENAI_API_KEY").unwrap_or_default(),
                model: env::var("OPENAI_MODEL")
                    .unwrap_or_else(|_| crate::embeddings::DEFAULT_EMBEDDING_MODEL.to_string()),
            },
            timeout_seconds: parse_u64("OPENAI_TIMEOUT_SECONDS", 30),
            max_batch_size: parse_usize("OPENAI_MAX_BATCH_SIZE", 16),
            max_retries: parse_usize("OPENAI_MAX_RETRIES", 3),
            base_url: env::var("OPENAI_BASE_URL").ok(),
        }),
        "voyage" => EmbeddingsRuntimeConfig::Voyage,
        "cohere" => EmbeddingsRuntimeConfig::Cohere,
        "local" => EmbeddingsRuntimeConfig::Local,
        _ => EmbeddingsRuntimeConfig::Disabled,
    }
}

fn parse_u64(key: &str, fallback: u64) -> u64 {
    env::var(key)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(fallback)
}

fn parse_usize(key: &str, fallback: usize) -> usize {
    env::var(key)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(fallback)
}
