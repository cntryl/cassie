use std::env;

use crate::embeddings::openai::OpenAiConfig;

#[path = "config/limits.rs"]
mod limits;
use limits::limits_from_env;
#[path = "config/native_memory.rs"]
mod native_memory;
#[path = "config/password.rs"]
mod password;
pub(crate) use password::validate_listener_password;
use password::{password_from_env, validate_bootstrap_password};
#[path = "config/tls.rs"]
mod tls;
use tls::validate_transport_tls_policy;
pub(crate) use tls::{validate_pgwire_listener_transport, validate_rest_listener_transport};
#[path = "config/switches.rs"]
mod switches;
pub use switches::{ExecutionResultCacheEnabled, OperatorSwitchingEnabled};

#[derive(Debug, thiserror::Error)]
pub enum CassieRuntimeConfigError {
    #[error("{key} must be a positive integer byte count")]
    InvalidNativeMemoryBytes { key: &'static str },

    #[error("{key} must be set before starting Cassie servers")]
    PasswordEnvironmentMissing { key: &'static str },

    #[error("{key} is set but is empty after trimming whitespace")]
    PasswordEnvironmentEmpty { key: &'static str },

    #[error("CASSIE_EMBEDDINGS_PROVIDER='{provider}' is not available; use disabled, fallback, openai, openai_compatible, tei, ollama, voyage, cohere, or local")]
    UnsupportedEmbeddingProvider { provider: String },

    #[error("known default bootstrap password is unsafe for non-loopback listener '{listener}'")]
    UnsafeDefaultPassword { listener: String },

    #[error("bootstrap password is empty for network listener '{listener}'")]
    EmptyBootstrapPassword { listener: String },

    #[error("bootstrap role '{role}' has no password for network listener '{listener}'")]
    PasswordlessBootstrapRole { role: String, listener: String },

    #[error("REST TLS certificate and key must be configured together")]
    RestTlsPair,

    #[error("REST TLS is required for non-loopback listener '{listener}'")]
    RestTlsRequired { listener: String },

    #[error("pgwire TLS certificate and key must be configured together")]
    PgwireTlsPair,

    #[error("pgwire TLS is required for non-loopback listener '{listener}'")]
    PgwireTlsRequired { listener: String },

    #[error("pgwire listener '{listener}' is not a valid socket address")]
    InvalidPgwireListener { listener: String },

    #[error("REST listener '{listener}' is not a valid socket address")]
    InvalidRestListener { listener: String },
}

#[derive(Debug, Clone)]
pub struct CassieRuntimeConfig {
    pub pgwire_listen: String,
    pub pgwire_tls_cert_file: Option<String>,
    pub pgwire_tls_key_file: Option<String>,
    pub rest_listen: String,
    pub rest_tls_cert_file: Option<String>,
    pub rest_tls_key_file: Option<String>,
    pub rest_external_https: bool,
    pub allow_insecure_non_loopback_listen: bool,
    pub admin_ui_dir: String,
    pub database: String,
    pub password: String,
    pub auth_user_attempts_per_minute: usize,
    pub auth_ip_attempts_per_minute: usize,
    pub auth_rate_limit_max_entries: usize,
    pub embeddings_max_response_bytes: usize,
    /// Optional native budget for explicit in-memory construction only.
    pub storage_memory_budget_bytes: Option<usize>,
    /// Optional native memtable limit for explicit in-memory construction only.
    pub storage_memtable_size_bytes: Option<usize>,
    pub limits: CassieRuntimeLimits,
    pub embeddings: EmbeddingsRuntimeConfig,
}

#[derive(Debug, Clone)]
pub struct CassieRuntimeLimits {
    pub query_timeout_ms: u64,
    pub max_result_rows: usize,
    pub cte_recursion_depth: usize,
    pub query_memory_budget_bytes: usize,
    pub repeatable_read_history_memory_budget_bytes: usize,
    pub execution_result_cache_enabled: ExecutionResultCacheEnabled,
    pub execution_result_cache_max_entries: usize,
    pub execution_result_cache_max_bytes: usize,
    pub plan_cache_entries: usize,
    pub cf2_plan_ttl_seconds: u64,
    pub cf2_plan_candidate_ttl_seconds: u64,
    pub cf2_fulltext_stats_ttl_seconds: u64,
    pub feedback_entries: usize,
    pub feedback_ttl_seconds: u64,
    pub operator_feedback_enabled: bool,
    pub vectorized_joins_enabled: bool,
    pub vectorized_join_batch_size: usize,
    pub adaptive_execution_enabled: bool,
    pub adaptive_min_cost_savings_bps: usize,
    pub adaptive_min_confidence_bps: u16,
    pub operator_switching_enabled: OperatorSwitchingEnabled,
    pub operator_switch_join_row_threshold: usize,
    pub adaptive_candidate_min: usize,
    pub adaptive_candidate_max: usize,
    pub parallel_scan_workers: usize,
    pub parallel_scoring_workers: usize,
    pub parallel_aggregation_workers: usize,
    pub max_query_workers: usize,
    pub pgwire_max_connections: usize,
    pub rest_max_connections: usize,
    pub rest_max_sessions_per_user: usize,
    pub rest_write_timeout_ms: u64,
    pub pgwire_write_timeout_ms: u64,
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
pub struct VoyageRuntimeConfig {
    pub api_key: String,
    pub model: String,
    pub dimensions: usize,
    pub timeout_seconds: u64,
    pub max_batch_size: usize,
    pub max_retries: usize,
    pub base_url: String,
}

#[derive(Debug, Clone)]
pub struct CohereRuntimeConfig {
    pub api_key: String,
    pub model: String,
    pub dimensions: usize,
    pub timeout_seconds: u64,
    pub max_batch_size: usize,
    pub max_retries: usize,
    pub base_url: String,
}

#[derive(Debug, Clone)]
pub struct LocalRuntimeConfig {
    pub model: String,
    pub dimensions: usize,
}

#[derive(Debug, Clone)]
pub enum EmbeddingsRuntimeConfig {
    Disabled,
    OpenAI(OpenAiRuntimeConfig),
    OpenAiCompatible(OpenAiCompatibleRuntimeConfig),
    Tei(SelfHostedEmbeddingRuntimeConfig),
    Ollama(SelfHostedEmbeddingRuntimeConfig),
    Voyage(VoyageRuntimeConfig),
    Cohere(CohereRuntimeConfig),
    Local(LocalRuntimeConfig),
}

impl Default for CassieRuntimeConfig {
    fn default() -> Self {
        Self {
            pgwire_listen: "127.0.0.1:5432".to_string(),
            pgwire_tls_cert_file: None,
            pgwire_tls_key_file: None,
            rest_listen: "127.0.0.1:8080".to_string(),
            rest_tls_cert_file: None,
            rest_tls_key_file: None,
            rest_external_https: false,
            allow_insecure_non_loopback_listen: false,
            admin_ui_dir: "./ui/dist".to_string(),
            database: "postgres".to_string(),
            password: String::new(),
            auth_user_attempts_per_minute: 10,
            auth_ip_attempts_per_minute: 60,
            auth_rate_limit_max_entries: 4_096,
            embeddings_max_response_bytes: 8 * 1024 * 1024,
            storage_memory_budget_bytes: None,
            storage_memtable_size_bytes: None,
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
            query_memory_budget_bytes: 10 * 1024 * 1024,
            repeatable_read_history_memory_budget_bytes: 10 * 1024 * 1024,
            execution_result_cache_enabled: ExecutionResultCacheEnabled::enabled(),
            execution_result_cache_max_entries: 64,
            execution_result_cache_max_bytes: 64 * 1024 * 1024,
            plan_cache_entries: 128,
            cf2_plan_ttl_seconds: 900,
            cf2_plan_candidate_ttl_seconds: 300,
            cf2_fulltext_stats_ttl_seconds: 300,
            feedback_entries: 128,
            feedback_ttl_seconds: 900,
            operator_feedback_enabled: false,
            vectorized_joins_enabled: false,
            vectorized_join_batch_size: 1024,
            adaptive_execution_enabled: false,
            adaptive_min_cost_savings_bps: 500,
            adaptive_min_confidence_bps: 0,
            operator_switching_enabled: OperatorSwitchingEnabled::disabled(),
            operator_switch_join_row_threshold: 4096,
            adaptive_candidate_min: 16,
            adaptive_candidate_max: 100_000,
            parallel_scan_workers: 1,
            parallel_scoring_workers: 1,
            parallel_aggregation_workers: 1,
            max_query_workers: 64,
            pgwire_max_connections: 256,
            rest_max_connections: 512,
            rest_max_sessions_per_user: 16,
            rest_write_timeout_ms: 10_000,
            pgwire_write_timeout_ms: 10_000,
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
    /// # Errors
    ///
    /// Returns an error when an environment-provided setting is invalid.
    pub fn from_env() -> Result<Self, CassieRuntimeConfigError> {
        Self::from_env_reader(|key| env::var(key).ok())
    }

    /// Loads configuration for the Cassie server binary and requires a root password.
    ///
    /// Embedded callers may use [`Self::from_env`] without configuring network authentication.
    ///
    /// # Errors
    ///
    /// Returns an error when the root password or other server configuration is invalid.
    pub fn from_server_env() -> Result<Self, CassieRuntimeConfigError> {
        Self::from_server_env_reader(|key| env::var(key).ok())
    }

    fn from_server_env_reader(
        env_reader: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, CassieRuntimeConfigError> {
        let config = Self::from_env_reader(&env_reader)?;
        if config.password.is_empty() {
            return Err(CassieRuntimeConfigError::PasswordEnvironmentMissing {
                key: "CASSIE_ROOT_PASSWORD",
            });
        }
        validate_bootstrap_password(&config)?;
        Ok(config)
    }

    fn from_env_reader(
        env_reader: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, CassieRuntimeConfigError> {
        let mut config = Self::default();
        if let Some(v) = env_reader("CASSIE_PGWIRE_LISTEN") {
            config.pgwire_listen = v;
        }
        if let Some(v) = env_reader("CASSIE_REST_LISTEN") {
            config.rest_listen = v;
        }
        config.pgwire_tls_cert_file = env_reader("CASSIE_PGWIRE_TLS_CERT_FILE");
        config.pgwire_tls_key_file = env_reader("CASSIE_PGWIRE_TLS_KEY_FILE");
        config.rest_tls_cert_file = env_reader("CASSIE_REST_TLS_CERT_FILE");
        config.rest_tls_key_file = env_reader("CASSIE_REST_TLS_KEY_FILE");
        config.rest_external_https =
            parse_bool_from(&env_reader, "CASSIE_REST_EXTERNAL_HTTPS", false);
        config.allow_insecure_non_loopback_listen = parse_bool_from(
            &env_reader,
            "CASSIE_ALLOW_INSECURE_NON_LOOPBACK_LISTEN",
            false,
        );
        if let Some(v) = env_reader("CASSIE_ADMIN_UI_DIR") {
            config.admin_ui_dir = v;
        }
        if let Some(v) = env_reader("CASSIE_DEFAULT_DATABASE") {
            config.database = v;
        }

        config.password = password_from_env(&env_reader)?;
        config.auth_user_attempts_per_minute = parse_usize_min_from(
            &env_reader,
            "CASSIE_AUTH_USER_ATTEMPTS_PER_MINUTE",
            config.auth_user_attempts_per_minute,
            1,
        );
        config.auth_ip_attempts_per_minute = parse_usize_min_from(
            &env_reader,
            "CASSIE_AUTH_IP_ATTEMPTS_PER_MINUTE",
            config.auth_ip_attempts_per_minute,
            1,
        );
        config.auth_rate_limit_max_entries = parse_usize_min_from(
            &env_reader,
            "CASSIE_AUTH_RATE_LIMIT_MAX_ENTRIES",
            config.auth_rate_limit_max_entries,
            1,
        );
        config.embeddings_max_response_bytes = parse_usize_min_from(
            &env_reader,
            "CASSIE_EMBEDDINGS_MAX_RESPONSE_BYTES",
            config.embeddings_max_response_bytes,
            1,
        );

        let provider =
            env_reader("CASSIE_EMBEDDINGS_PROVIDER").unwrap_or_else(|| "disabled".to_string());
        let provider = provider.trim().to_ascii_lowercase();
        config.embeddings = match provider.as_str() {
            "disabled" | "fallback" | "openai" | "openai_compatible" | "tei" | "ollama"
            | "voyage" | "cohere" | "local" => {
                parse_provider_config_from(provider.as_str(), &env_reader)
            }
            _ => {
                return Err(CassieRuntimeConfigError::UnsupportedEmbeddingProvider { provider });
            }
        };
        config.limits = limits_from_env(&env_reader, &config.limits);
        config.storage_memory_budget_bytes = native_memory::optional_positive_bytes(
            &env_reader,
            "CASSIE_STORAGE_MEMORY_BUDGET_BYTES",
        )?;
        config.storage_memtable_size_bytes = native_memory::optional_positive_bytes(
            &env_reader,
            "CASSIE_STORAGE_MEMTABLE_SIZE_BYTES",
        )?;

        validate_transport_tls_policy(&config)?;

        Ok(config)
    }
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
        "voyage" => EmbeddingsRuntimeConfig::Voyage(VoyageRuntimeConfig {
            api_key: env_reader("CASSIE_VOYAGE_API_KEY").unwrap_or_default(),
            model: env_reader("CASSIE_VOYAGE_MODEL")
                .unwrap_or_else(|| "voyage-3.5-lite".to_string()),
            dimensions: parse_usize_from(&env_reader, "CASSIE_VOYAGE_DIMENSIONS", 1024),
            timeout_seconds: parse_u64_from(&env_reader, "CASSIE_VOYAGE_TIMEOUT_SECONDS", 30),
            max_batch_size: parse_usize_from(&env_reader, "CASSIE_VOYAGE_MAX_BATCH_SIZE", 16),
            max_retries: parse_usize_from(&env_reader, "CASSIE_VOYAGE_MAX_RETRIES", 3),
            base_url: env_reader("CASSIE_VOYAGE_BASE_URL")
                .unwrap_or_else(|| "https://api.voyageai.com".to_string()),
        }),
        "cohere" => EmbeddingsRuntimeConfig::Cohere(CohereRuntimeConfig {
            api_key: env_reader("CASSIE_COHERE_API_KEY").unwrap_or_default(),
            model: env_reader("CASSIE_COHERE_MODEL").unwrap_or_else(|| "embed-v4.0".to_string()),
            dimensions: parse_usize_from(&env_reader, "CASSIE_COHERE_DIMENSIONS", 1536),
            timeout_seconds: parse_u64_from(&env_reader, "CASSIE_COHERE_TIMEOUT_SECONDS", 30),
            max_batch_size: parse_usize_from(&env_reader, "CASSIE_COHERE_MAX_BATCH_SIZE", 96),
            max_retries: parse_usize_from(&env_reader, "CASSIE_COHERE_MAX_RETRIES", 3),
            base_url: env_reader("CASSIE_COHERE_BASE_URL")
                .unwrap_or_else(|| "https://api.cohere.com".to_string()),
        }),
        "local" => EmbeddingsRuntimeConfig::Local(LocalRuntimeConfig {
            model: env_reader("CASSIE_LOCAL_MODEL")
                .unwrap_or_else(|| "cassie-local-hash-v1".to_string()),
            dimensions: parse_usize_from(&env_reader, "CASSIE_LOCAL_DIMENSIONS", 384),
        }),
        _ => EmbeddingsRuntimeConfig::Disabled,
    }
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

fn parse_usize_min_from(
    env_reader: &impl Fn(&str) -> Option<String>,
    key: &str,
    fallback: usize,
    min: usize,
) -> usize {
    parse_usize_from(env_reader, key, fallback).max(min)
}

fn parse_u16_from(env_reader: &impl Fn(&str) -> Option<String>, key: &str, fallback: u16) -> u16 {
    env_reader(key)
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(fallback)
}

fn parse_bool_from(
    env_reader: &impl Fn(&str) -> Option<String>,
    key: &str,
    fallback: bool,
) -> bool {
    env_reader(key)
        .and_then(|value| match value.to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        })
        .unwrap_or(fallback)
}

fn parse_operator_switching_enabled_from(
    env_reader: &impl Fn(&str) -> Option<String>,
    fallback: OperatorSwitchingEnabled,
) -> OperatorSwitchingEnabled {
    if parse_bool_from(
        env_reader,
        "CASSIE_OPERATOR_SWITCHING_ENABLED",
        fallback.is_enabled(),
    ) {
        OperatorSwitchingEnabled::enabled()
    } else {
        OperatorSwitchingEnabled::disabled()
    }
}

#[cfg(test)]
mod tests;
