use std::env;

use crate::embeddings::openai::OpenAiConfig;

#[derive(Debug, Clone)]
pub struct CassieRuntimeConfig {
    pub pgwire_listen: String,
    pub rest_listen: String,
    pub user: String,
    pub database: String,
    pub password: String,
    pub limits: CassieRuntimeLimits,
    pub embeddings: EmbeddingsRuntimeConfig,
}

#[derive(Debug, Clone)]
pub struct CassieRuntimeLimits {
    pub query_timeout_ms: u64,
    pub max_result_rows: usize,
    pub cte_recursion_depth: usize,
    pub temp_spill_budget_bytes: usize,
    pub plan_cache_entries: usize,
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
            database: "postgres".to_string(),
            password: "postgres".to_string(),
            limits: CassieRuntimeLimits::default(),
            embeddings: EmbeddingsRuntimeConfig::Disabled,
        }
    }
}

impl Default for CassieRuntimeLimits {
    fn default() -> Self {
        Self {
            query_timeout_ms: 30_000,
            max_result_rows: 100_000,
            cte_recursion_depth: 64,
            temp_spill_budget_bytes: 10 * 1024 * 1024,
            plan_cache_entries: 128,
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
        if let Ok(v) = env::var("CASSIE_ADMIN_USER") {
            config.user = v;
        }
        if let Ok(v) = env::var("CASSIE_DEFAULT_DATABASE") {
            config.database = v;
        }

        if let Some(v) = read_password_from_file() {
            config.password = v;
        } else if let Ok(v) = env::var("CASSIE_ADMIN_PASSWORD") {
            config.password = v;
        }

        let provider =
            env::var("CASSIE_EMBEDDINGS_PROVIDER").unwrap_or_else(|_| "disabled".to_string());
        config.embeddings = parse_provider_config(provider.to_lowercase().as_str());
        config.limits = CassieRuntimeLimits {
            query_timeout_ms: parse_u64("CASSIE_QUERY_TIMEOUT_MS", config.limits.query_timeout_ms),
            max_result_rows: parse_usize("CASSIE_MAX_RESULT_ROWS", config.limits.max_result_rows),
            cte_recursion_depth: parse_usize(
                "CASSIE_CTE_RECURSION_DEPTH",
                config.limits.cte_recursion_depth,
            ),
            temp_spill_budget_bytes: parse_usize(
                "CASSIE_TEMP_SPILL_BUDGET_BYTES",
                config.limits.temp_spill_budget_bytes,
            ),
            plan_cache_entries: parse_usize(
                "CASSIE_PLAN_CACHE_ENTRIES",
                config.limits.plan_cache_entries,
            ),
        };

        config
    }
}

fn read_password_from_file() -> Option<String> {
    let path = env::var("CASSIE_ADMIN_PASSWORD_FILE").ok()?;
    let value = std::fs::read_to_string(path).ok()?;
    Some(value.trim().to_string())
}

fn parse_provider_config(provider: &str) -> EmbeddingsRuntimeConfig {
    match provider {
        "openai" => EmbeddingsRuntimeConfig::OpenAI(OpenAiRuntimeConfig {
            config: OpenAiConfig {
                api_key: env::var("CASSIE_OPENAI_API_KEY").unwrap_or_default(),
                model: env::var("CASSIE_OPENAI_MODEL")
                    .unwrap_or_else(|_| crate::embeddings::DEFAULT_EMBEDDING_MODEL.to_string()),
            },
            timeout_seconds: parse_u64("CASSIE_OPENAI_TIMEOUT_SECONDS", 30),
            max_batch_size: parse_usize("CASSIE_OPENAI_MAX_BATCH_SIZE", 16),
            max_retries: parse_usize("CASSIE_OPENAI_MAX_RETRIES", 3),
            base_url: env::var("CASSIE_OPENAI_BASE_URL").ok(),
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
