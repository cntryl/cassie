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
    pub cf2_plan_ttl_seconds: u64,
    pub cf2_plan_candidate_ttl_seconds: u64,
    pub cf2_fulltext_stats_ttl_seconds: u64,
    pub feedback_entries: usize,
    pub feedback_ttl_seconds: u64,
    pub operator_feedback_enabled: bool,
    pub experimental_column_store_enabled: bool,
    pub vectorized_joins_enabled: bool,
    pub vectorized_join_batch_size: usize,
    pub adaptive_candidate_min: usize,
    pub adaptive_candidate_max: usize,
    pub parallel_scan_workers: usize,
    pub parallel_scoring_workers: usize,
    pub parallel_aggregation_workers: usize,
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
pub struct SelfHostedEmbeddingRuntimeConfig {
    pub base_url: String,
    pub model: String,
    pub dimensions: usize,
    pub timeout_seconds: u64,
    pub max_batch_size: usize,
    pub max_retries: usize,
}

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleRuntimeConfig {
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
    pub dimensions: usize,
    pub timeout_seconds: u64,
    pub max_batch_size: usize,
    pub max_retries: usize,
}

#[derive(Debug, Clone)]
pub enum EmbeddingsRuntimeConfig {
    Disabled,
    OpenAI(OpenAiRuntimeConfig),
    OpenAiCompatible(OpenAiCompatibleRuntimeConfig),
    Tei(SelfHostedEmbeddingRuntimeConfig),
    Ollama(SelfHostedEmbeddingRuntimeConfig),
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
            cf2_plan_ttl_seconds: 900,
            cf2_plan_candidate_ttl_seconds: 300,
            cf2_fulltext_stats_ttl_seconds: 300,
            feedback_entries: 128,
            feedback_ttl_seconds: 900,
            operator_feedback_enabled: false,
            experimental_column_store_enabled: false,
            vectorized_joins_enabled: false,
            vectorized_join_batch_size: 1024,
            adaptive_candidate_min: 16,
            adaptive_candidate_max: 100_000,
            parallel_scan_workers: 1,
            parallel_scoring_workers: 1,
            parallel_aggregation_workers: 1,
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
            cf2_plan_ttl_seconds: parse_u64(
                "CASSIE_CF2_PLAN_TTL_SECONDS",
                config.limits.cf2_plan_ttl_seconds,
            ),
            cf2_plan_candidate_ttl_seconds: parse_u64(
                "CASSIE_CF2_PLAN_CANDIDATE_TTL_SECONDS",
                config.limits.cf2_plan_candidate_ttl_seconds,
            ),
            cf2_fulltext_stats_ttl_seconds: parse_u64(
                "CASSIE_CF2_FULLTEXT_STATS_TTL_SECONDS",
                config.limits.cf2_fulltext_stats_ttl_seconds,
            ),
            feedback_entries: parse_usize(
                "CASSIE_FEEDBACK_ENTRIES",
                config.limits.feedback_entries,
            ),
            feedback_ttl_seconds: parse_u64(
                "CASSIE_FEEDBACK_TTL_SECONDS",
                config.limits.feedback_ttl_seconds,
            ),
            operator_feedback_enabled: parse_bool(
                "CASSIE_OPERATOR_FEEDBACK_ENABLED",
                config.limits.operator_feedback_enabled,
            ),
            experimental_column_store_enabled: parse_bool(
                "CASSIE_EXPERIMENTAL_COLUMN_STORE_ENABLED",
                config.limits.experimental_column_store_enabled,
            ),
            vectorized_joins_enabled: parse_bool(
                "CASSIE_VECTORIZED_JOINS_ENABLED",
                config.limits.vectorized_joins_enabled,
            ),
            vectorized_join_batch_size: parse_usize(
                "CASSIE_VECTORIZED_JOIN_BATCH_SIZE",
                config.limits.vectorized_join_batch_size,
            ),
            adaptive_candidate_min: parse_usize(
                "CASSIE_ADAPTIVE_CANDIDATE_MIN",
                config.limits.adaptive_candidate_min,
            ),
            adaptive_candidate_max: parse_usize(
                "CASSIE_ADAPTIVE_CANDIDATE_MAX",
                config.limits.adaptive_candidate_max,
            ),
            parallel_scan_workers: parse_usize(
                "CASSIE_PARALLEL_SCAN_WORKERS",
                config.limits.parallel_scan_workers,
            ),
            parallel_scoring_workers: parse_usize(
                "CASSIE_PARALLEL_SCORING_WORKERS",
                config.limits.parallel_scoring_workers,
            ),
            parallel_aggregation_workers: parse_usize(
                "CASSIE_PARALLEL_AGGREGATION_WORKERS",
                config.limits.parallel_aggregation_workers,
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
    parse_provider_config_from(provider, |key| env::var(key).ok())
}

fn parse_provider_config_from(
    provider: &str,
    env_reader: impl Fn(&str) -> Option<String>,
) -> EmbeddingsRuntimeConfig {
    match provider {
        "openai" => EmbeddingsRuntimeConfig::OpenAI(OpenAiRuntimeConfig {
            config: OpenAiConfig {
                api_key: env_reader("CASSIE_OPENAI_API_KEY").unwrap_or_default(),
                model: env_reader("CASSIE_OPENAI_MODEL")
                    .unwrap_or_else(|| crate::embeddings::DEFAULT_EMBEDDING_MODEL.to_string()),
            },
            timeout_seconds: parse_u64_from(&env_reader, "CASSIE_OPENAI_TIMEOUT_SECONDS", 30),
            max_batch_size: parse_usize_from(&env_reader, "CASSIE_OPENAI_MAX_BATCH_SIZE", 16),
            max_retries: parse_usize_from(&env_reader, "CASSIE_OPENAI_MAX_RETRIES", 3),
            base_url: env_reader("CASSIE_OPENAI_BASE_URL"),
        }),
        "openai_compatible" => {
            EmbeddingsRuntimeConfig::OpenAiCompatible(OpenAiCompatibleRuntimeConfig {
                base_url: env_reader("CASSIE_EMBEDDINGS_BASE_URL").unwrap_or_default(),
                api_key: env_reader("CASSIE_EMBEDDINGS_API_KEY").filter(|value| !value.is_empty()),
                model: env_reader("CASSIE_EMBEDDINGS_MODEL")
                    .unwrap_or_else(|| "BAAI/bge-small-en-v1.5".to_string()),
                dimensions: parse_usize_from(&env_reader, "CASSIE_EMBEDDINGS_DIMENSIONS", 384),
                timeout_seconds: parse_u64_from(
                    &env_reader,
                    "CASSIE_EMBEDDINGS_TIMEOUT_SECONDS",
                    30,
                ),
                max_batch_size: parse_usize_from(
                    &env_reader,
                    "CASSIE_EMBEDDINGS_MAX_BATCH_SIZE",
                    16,
                ),
                max_retries: parse_usize_from(&env_reader, "CASSIE_EMBEDDINGS_MAX_RETRIES", 3),
            })
        }
        "tei" => EmbeddingsRuntimeConfig::Tei(SelfHostedEmbeddingRuntimeConfig {
            base_url: env_reader("CASSIE_TEI_BASE_URL")
                .unwrap_or_else(|| "http://127.0.0.1:8080".to_string()),
            model: env_reader("CASSIE_TEI_MODEL")
                .unwrap_or_else(|| "BAAI/bge-small-en-v1.5".to_string()),
            dimensions: parse_usize_from(&env_reader, "CASSIE_TEI_DIMENSIONS", 384),
            timeout_seconds: parse_u64_from(&env_reader, "CASSIE_TEI_TIMEOUT_SECONDS", 30),
            max_batch_size: parse_usize_from(&env_reader, "CASSIE_TEI_MAX_BATCH_SIZE", 32),
            max_retries: parse_usize_from(&env_reader, "CASSIE_TEI_MAX_RETRIES", 3),
        }),
        "ollama" => EmbeddingsRuntimeConfig::Ollama(SelfHostedEmbeddingRuntimeConfig {
            base_url: env_reader("CASSIE_OLLAMA_BASE_URL")
                .unwrap_or_else(|| "http://127.0.0.1:11434".to_string()),
            model: env_reader("CASSIE_OLLAMA_MODEL")
                .unwrap_or_else(|| "nomic-embed-text".to_string()),
            dimensions: parse_usize_from(&env_reader, "CASSIE_OLLAMA_DIMENSIONS", 768),
            timeout_seconds: parse_u64_from(&env_reader, "CASSIE_OLLAMA_TIMEOUT_SECONDS", 30),
            max_batch_size: parse_usize_from(&env_reader, "CASSIE_OLLAMA_MAX_BATCH_SIZE", 16),
            max_retries: parse_usize_from(&env_reader, "CASSIE_OLLAMA_MAX_RETRIES", 3),
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

fn parse_bool(key: &str, fallback: bool) -> bool {
    env::var(key)
        .ok()
        .and_then(|value| match value.to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        })
        .unwrap_or(fallback)
}

fn parse_u64_from(env_reader: &impl Fn(&str) -> Option<String>, key: &str, fallback: u64) -> u64 {
    env_reader(key)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(fallback)
}

fn parse_usize_from(
    env_reader: &impl Fn(&str) -> Option<String>,
    key: &str,
    fallback: usize,
) -> usize {
    env_reader(key)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env_reader(values: HashMap<&'static str, &'static str>) -> impl Fn(&str) -> Option<String> {
        move |key| values.get(key).map(|value| value.to_string())
    }

    #[test]
    fn should_parse_tei_embedding_runtime_config() {
        // Arrange
        let values = HashMap::from([
            ("CASSIE_TEI_BASE_URL", "http://tei.local:8080"),
            ("CASSIE_TEI_MODEL", "BAAI/bge-base-en-v1.5"),
            ("CASSIE_TEI_DIMENSIONS", "768"),
            ("CASSIE_TEI_TIMEOUT_SECONDS", "45"),
            ("CASSIE_TEI_MAX_BATCH_SIZE", "64"),
            ("CASSIE_TEI_MAX_RETRIES", "5"),
        ]);

        // Act
        let config = parse_provider_config_from("tei", env_reader(values));

        // Assert
        match config {
            EmbeddingsRuntimeConfig::Tei(runtime) => {
                assert_eq!(runtime.base_url, "http://tei.local:8080");
                assert_eq!(runtime.model, "BAAI/bge-base-en-v1.5");
                assert_eq!(runtime.dimensions, 768);
                assert_eq!(runtime.timeout_seconds, 45);
                assert_eq!(runtime.max_batch_size, 64);
                assert_eq!(runtime.max_retries, 5);
            }
            _ => panic!("expected tei config"),
        }
    }

    #[test]
    fn should_parse_openai_compatible_embedding_runtime_config() {
        // Arrange
        let values = HashMap::from([
            ("CASSIE_EMBEDDINGS_BASE_URL", "http://embed.local:9000"),
            ("CASSIE_EMBEDDINGS_MODEL", "custom-model"),
            ("CASSIE_EMBEDDINGS_DIMENSIONS", "1024"),
            ("CASSIE_EMBEDDINGS_API_KEY", "secret"),
            ("CASSIE_EMBEDDINGS_TIMEOUT_SECONDS", "12"),
            ("CASSIE_EMBEDDINGS_MAX_BATCH_SIZE", "24"),
            ("CASSIE_EMBEDDINGS_MAX_RETRIES", "4"),
        ]);

        // Act
        let config = parse_provider_config_from("openai_compatible", env_reader(values));

        // Assert
        match config {
            EmbeddingsRuntimeConfig::OpenAiCompatible(runtime) => {
                assert_eq!(runtime.base_url, "http://embed.local:9000");
                assert_eq!(runtime.model, "custom-model");
                assert_eq!(runtime.dimensions, 1024);
                assert_eq!(runtime.api_key.as_deref(), Some("secret"));
                assert_eq!(runtime.timeout_seconds, 12);
                assert_eq!(runtime.max_batch_size, 24);
                assert_eq!(runtime.max_retries, 4);
            }
            _ => panic!("expected openai compatible config"),
        }
    }

    #[test]
    fn should_parse_ollama_embedding_runtime_config() {
        // Arrange
        let values = HashMap::from([
            ("CASSIE_OLLAMA_BASE_URL", "http://ollama.local:11434"),
            ("CASSIE_OLLAMA_MODEL", "embeddinggemma"),
            ("CASSIE_OLLAMA_DIMENSIONS", "768"),
            ("CASSIE_OLLAMA_TIMEOUT_SECONDS", "20"),
            ("CASSIE_OLLAMA_MAX_BATCH_SIZE", "12"),
            ("CASSIE_OLLAMA_MAX_RETRIES", "6"),
        ]);

        // Act
        let config = parse_provider_config_from("ollama", env_reader(values));

        // Assert
        match config {
            EmbeddingsRuntimeConfig::Ollama(runtime) => {
                assert_eq!(runtime.base_url, "http://ollama.local:11434");
                assert_eq!(runtime.model, "embeddinggemma");
                assert_eq!(runtime.dimensions, 768);
                assert_eq!(runtime.timeout_seconds, 20);
                assert_eq!(runtime.max_batch_size, 12);
                assert_eq!(runtime.max_retries, 6);
            }
            _ => panic!("expected ollama config"),
        }
    }
}
