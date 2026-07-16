use std::env;

use crate::embeddings::openai::OpenAiConfig;

#[path = "config/limits.rs"]
mod limits;
use limits::limits_from_env;
#[path = "config/tls.rs"]
mod tls;
use tls::{validate_bootstrap_password, validate_transport_tls_policy};
#[path = "config/switches.rs"]
mod switches;
pub use switches::{ExecutionResultCacheEnabled, OperatorSwitchingEnabled};

#[derive(Debug, thiserror::Error)]
pub enum CassieRuntimeConfigError {
    #[error("{key} is set but could not be read from '{path}': {source}")]
    PasswordFileRead {
        key: &'static str,
        path: String,
        source: std::io::Error,
    },

    #[error("{key} is set but '{path}' is empty after trimming whitespace")]
    PasswordFileEmpty { key: &'static str, path: String },

    #[error("CASSIE_EMBEDDINGS_PROVIDER='{provider}' is not available; use disabled, fallback, openai, openai_compatible, tei, ollama, voyage, cohere, or local")]
    UnsupportedEmbeddingProvider { provider: String },

    #[error("default bootstrap password is unsafe for non-loopback listener '{listener}'")]
    UnsafeDefaultPassword { listener: String },

    #[error("REST TLS certificate and key must be configured together")]
    RestTlsPair,

    #[error("REST TLS is required for non-loopback listener '{listener}'")]
    RestTlsRequired { listener: String },

    #[error("pgwire TLS certificate and key must be configured together")]
    PgwireTlsPair,

    #[error("pgwire TLS is required for non-loopback listener '{listener}'")]
    PgwireTlsRequired { listener: String },
}

#[derive(Debug, Clone)]
pub struct CassieRuntimeConfig {
    pub pgwire_listen: String,
    pub pgwire_tls_cert_file: Option<String>,
    pub pgwire_tls_key_file: Option<String>,
    pub rest_listen: String,
    pub rest_tls_cert_file: Option<String>,
    pub rest_tls_key_file: Option<String>,
    pub admin_ui_dir: String,
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
    pub query_memory_budget_bytes: usize,
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
            admin_ui_dir: "./ui/dist".to_string(),
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
            query_memory_budget_bytes: 10 * 1024 * 1024,
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
            adaptive_execution_enabled: true,
            adaptive_min_cost_savings_bps: 500,
            adaptive_min_confidence_bps: 0,
            operator_switching_enabled: OperatorSwitchingEnabled::enabled(),
            operator_switch_join_row_threshold: 4096,
            adaptive_candidate_min: 16,
            adaptive_candidate_max: 100_000,
            parallel_scan_workers: 1,
            parallel_scoring_workers: 1,
            parallel_aggregation_workers: 1,
            max_query_workers: 64,
            pgwire_max_connections: 256,
            rest_max_connections: 512,
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
    /// Returns an error when validation, storage, or execution fails.
    pub fn from_env() -> Result<Self, CassieRuntimeConfigError> {
        Self::from_env_reader(|key| env::var(key).ok())
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
        if let Some(v) = env_reader("CASSIE_ADMIN_UI_DIR") {
            config.admin_ui_dir = v;
        }
        if let Some(v) = env_reader("CASSIE_ADMIN_USER") {
            config.user = v;
        }
        if let Some(v) = env_reader("CASSIE_DEFAULT_DATABASE") {
            config.database = v;
        }

        if let Some(v) = read_password_from_file_from(&env_reader)? {
            config.password = v;
        } else if let Some(v) = env_reader("CASSIE_ADMIN_PASSWORD") {
            config.password = v;
        }

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

        validate_bootstrap_password(&config)?;
        validate_transport_tls_policy(&config)?;

        Ok(config)
    }
}

fn read_password_from_file_from(
    env_reader: &impl Fn(&str) -> Option<String>,
) -> Result<Option<String>, CassieRuntimeConfigError> {
    let Some(path) = env_reader("CASSIE_ADMIN_PASSWORD_FILE") else {
        return Ok(None);
    };
    let value = std::fs::read_to_string(&path).map_err(|source| {
        CassieRuntimeConfigError::PasswordFileRead {
            key: "CASSIE_ADMIN_PASSWORD_FILE",
            path: path.clone(),
            source,
        }
    })?;
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(CassieRuntimeConfigError::PasswordFileEmpty {
            key: "CASSIE_ADMIN_PASSWORD_FILE",
            path,
        });
    }
    Ok(Some(value))
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
mod tests {
    use super::*;
    use std::collections::HashMap;
    use uuid::Uuid;

    fn env_reader(values: HashMap<&'static str, &'static str>) -> impl Fn(&str) -> Option<String> {
        move |key| values.get(key).map(std::string::ToString::to_string)
    }

    fn env_reader_owned(values: HashMap<&'static str, String>) -> impl Fn(&str) -> Option<String> {
        move |key| values.get(key).cloned()
    }

    fn temp_file(label: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("cassie-config-{label}-{}", Uuid::new_v4()));
        path
    }

    #[test]
    fn should_use_admin_password_file_before_admin_password_env() {
        // Arrange
        let path = temp_file("password-file-precedence");
        std::fs::write(&path, " file-secret \n").expect("write password file");
        let values = HashMap::from([
            (
                "CASSIE_ADMIN_PASSWORD_FILE",
                path.to_string_lossy().to_string(),
            ),
            ("CASSIE_ADMIN_PASSWORD", "env-secret".to_string()),
        ]);

        // Act
        let config =
            CassieRuntimeConfig::from_env_reader(env_reader_owned(values)).expect("runtime config");

        // Assert
        assert_eq!(config.password, "file-secret");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn should_reject_missing_admin_password_file_without_fallback() {
        // Arrange
        let path = temp_file("missing-password-file");
        let values = HashMap::from([
            (
                "CASSIE_ADMIN_PASSWORD_FILE",
                path.to_string_lossy().to_string(),
            ),
            ("CASSIE_ADMIN_PASSWORD", "env-secret".to_string()),
        ]);

        // Act
        let error = CassieRuntimeConfig::from_env_reader(env_reader_owned(values))
            .expect_err("missing password file should fail");

        // Assert
        assert!(error.to_string().contains("CASSIE_ADMIN_PASSWORD_FILE"));
        assert!(error.to_string().contains(path.to_string_lossy().as_ref()));
    }

    #[test]
    fn should_reject_empty_admin_password_file_without_fallback() {
        // Arrange
        let path = temp_file("empty-password-file");
        std::fs::write(&path, " \n\t").expect("write password file");
        let values = HashMap::from([
            (
                "CASSIE_ADMIN_PASSWORD_FILE",
                path.to_string_lossy().to_string(),
            ),
            ("CASSIE_ADMIN_PASSWORD", "env-secret".to_string()),
        ]);

        // Act
        let error = CassieRuntimeConfig::from_env_reader(env_reader_owned(values))
            .expect_err("empty password file should fail");

        // Assert
        assert!(error.to_string().contains("empty"));
        assert!(error.to_string().contains(path.to_string_lossy().as_ref()));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn should_accept_explicit_embedding_providers_with_runtime_variants() {
        // Arrange
        let providers = ["voyage", "cohere", "local"];

        // Act
        let configs = providers.map(|provider| {
            let values = HashMap::from([("CASSIE_EMBEDDINGS_PROVIDER", provider.to_string())]);
            CassieRuntimeConfig::from_env_reader(env_reader_owned(values))
                .expect("explicit provider should parse")
        });

        // Assert
        for (provider, config) in providers.into_iter().zip(configs) {
            match (provider, config.embeddings) {
                ("voyage", EmbeddingsRuntimeConfig::Voyage(_))
                | ("cohere", EmbeddingsRuntimeConfig::Cohere(_))
                | ("local", EmbeddingsRuntimeConfig::Local(_)) => {}
                _ => panic!("expected runtime config for provider {provider}"),
            }
        }
    }

    #[test]
    fn should_keep_disabled_or_omitted_embedding_provider_available() {
        // Arrange
        let omitted = HashMap::new();
        let disabled = HashMap::from([("CASSIE_EMBEDDINGS_PROVIDER", "disabled".to_string())]);
        let fallback = HashMap::from([("CASSIE_EMBEDDINGS_PROVIDER", "fallback".to_string())]);

        // Act
        let omitted_config =
            CassieRuntimeConfig::from_env_reader(env_reader_owned(omitted)).expect("omitted");
        let disabled_config =
            CassieRuntimeConfig::from_env_reader(env_reader_owned(disabled)).expect("disabled");
        let fallback_config =
            CassieRuntimeConfig::from_env_reader(env_reader_owned(fallback)).expect("fallback");

        // Assert
        assert!(matches!(
            omitted_config.embeddings,
            EmbeddingsRuntimeConfig::Disabled
        ));
        assert!(matches!(
            disabled_config.embeddings,
            EmbeddingsRuntimeConfig::Disabled
        ));
        assert!(matches!(
            fallback_config.embeddings,
            EmbeddingsRuntimeConfig::Disabled
        ));
    }

    #[test]
    fn should_reject_default_password_on_non_loopback_listener() {
        // Arrange
        let values = HashMap::from([("CASSIE_PGWIRE_LISTEN", "0.0.0.0:5432")]);

        // Act
        let error = CassieRuntimeConfig::from_env_reader(env_reader(values))
            .expect_err("default password must not expose a non-loopback listener");

        // Assert
        assert!(error.to_string().contains("unsafe"));
        assert!(error.to_string().contains("0.0.0.0:5432"));
    }

    #[test]
    fn should_allow_explicit_password_and_tls_on_non_loopback_listener() {
        // Arrange
        let values = HashMap::from([
            ("CASSIE_PGWIRE_LISTEN", "0.0.0.0:5432"),
            ("CASSIE_ADMIN_PASSWORD", "different-secret"),
            ("CASSIE_PGWIRE_TLS_CERT_FILE", "/etc/cassie/tls/cert.pem"),
            ("CASSIE_PGWIRE_TLS_KEY_FILE", "/etc/cassie/tls/key.pem"),
        ]);

        // Act
        let config = CassieRuntimeConfig::from_env_reader(env_reader(values))
            .expect("explicit password should permit non-loopback listener");

        // Assert
        assert_eq!(config.password, "different-secret");
    }

    #[test]
    fn should_require_pgwire_tls_for_non_loopback_listener() {
        // Arrange
        let values = HashMap::from([
            ("CASSIE_PGWIRE_LISTEN", "0.0.0.0:5432"),
            ("CASSIE_ADMIN_PASSWORD", "different-secret"),
        ]);

        // Act
        let error = CassieRuntimeConfig::from_env_reader(env_reader(values))
            .expect_err("non-loopback pgwire must require TLS");

        // Assert
        assert!(error.to_string().contains("pgwire TLS"));
        assert!(error.to_string().contains("0.0.0.0:5432"));
    }

    #[test]
    fn should_reject_partial_pgwire_tls_configuration() {
        // Arrange
        let values = HashMap::from([("CASSIE_PGWIRE_TLS_CERT_FILE", "/tmp/cassie-cert.pem")]);

        // Act
        let error = CassieRuntimeConfig::from_env_reader(env_reader(values))
            .expect_err("pgwire TLS requires both certificate and key");

        // Assert
        assert!(error.to_string().contains("certificate and key"));
    }

    #[test]
    fn should_parse_pgwire_tls_file_paths() {
        // Arrange
        let values = HashMap::from([
            ("CASSIE_PGWIRE_TLS_CERT_FILE", "/etc/cassie/tls/cert.pem"),
            ("CASSIE_PGWIRE_TLS_KEY_FILE", "/etc/cassie/tls/key.pem"),
        ]);

        // Act
        let config =
            CassieRuntimeConfig::from_env_reader(env_reader(values)).expect("pgwire TLS paths");

        // Assert
        assert_eq!(
            config.pgwire_tls_cert_file.as_deref(),
            Some("/etc/cassie/tls/cert.pem")
        );
        assert_eq!(
            config.pgwire_tls_key_file.as_deref(),
            Some("/etc/cassie/tls/key.pem")
        );
    }

    #[test]
    fn should_require_rest_tls_for_non_loopback_listener() {
        // Arrange
        let values = HashMap::from([
            ("CASSIE_REST_LISTEN", "0.0.0.0:8080"),
            ("CASSIE_ADMIN_PASSWORD", "different-secret"),
        ]);

        // Act
        let error = CassieRuntimeConfig::from_env_reader(env_reader(values))
            .expect_err("non-loopback REST must require TLS");

        // Assert
        assert!(error.to_string().contains("REST TLS"));
        assert!(error.to_string().contains("0.0.0.0:8080"));
    }

    #[test]
    fn should_reject_partial_rest_tls_configuration() {
        // Arrange
        let values = HashMap::from([("CASSIE_REST_TLS_CERT_FILE", "/tmp/cassie-cert.pem")]);

        // Act
        let error = CassieRuntimeConfig::from_env_reader(env_reader(values))
            .expect_err("REST TLS requires both certificate and key");

        // Assert
        assert!(error.to_string().contains("certificate and key"));
    }

    #[test]
    fn should_parse_rest_tls_file_paths() {
        // Arrange
        let values = HashMap::from([
            ("CASSIE_REST_TLS_CERT_FILE", "/etc/cassie/tls/cert.pem"),
            ("CASSIE_REST_TLS_KEY_FILE", "/etc/cassie/tls/key.pem"),
        ]);

        // Act
        let config =
            CassieRuntimeConfig::from_env_reader(env_reader(values)).expect("REST TLS paths");

        // Assert
        assert_eq!(
            config.rest_tls_cert_file.as_deref(),
            Some("/etc/cassie/tls/cert.pem")
        );
        assert_eq!(
            config.rest_tls_key_file.as_deref(),
            Some("/etc/cassie/tls/key.pem")
        );
    }

    #[test]
    fn should_clamp_connection_admission_limits() {
        // Arrange
        let values = HashMap::from([
            ("CASSIE_PGWIRE_MAX_CONNECTIONS", "0".to_string()),
            ("CASSIE_REST_MAX_CONNECTIONS", "7".to_string()),
        ]);

        // Act
        let config =
            CassieRuntimeConfig::from_env_reader(env_reader_owned(values)).expect("runtime config");

        // Assert
        assert_eq!(config.limits.pgwire_max_connections, 1);
        assert_eq!(config.limits.rest_max_connections, 7);
    }

    #[test]
    fn should_parse_execution_result_cache_limits() {
        // Arrange
        let values = HashMap::from([
            ("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false"),
            ("CASSIE_EXECUTION_RESULT_CACHE_MAX_ENTRIES", "7"),
            ("CASSIE_EXECUTION_RESULT_CACHE_MAX_BYTES", "4096"),
        ]);

        // Act
        let config =
            CassieRuntimeConfig::from_env_reader(env_reader(values)).expect("runtime config");

        // Assert
        assert!(!config.limits.execution_result_cache_enabled.is_enabled());
        assert_eq!(config.limits.execution_result_cache_max_entries, 7);
        assert_eq!(config.limits.execution_result_cache_max_bytes, 4096);
    }

    #[test]
    fn should_use_only_query_memory_budget_environment_name() {
        // Arrange
        let current = HashMap::from([("CASSIE_QUERY_MEMORY_BUDGET_BYTES", "4096")]);
        let obsolete = HashMap::from([("CASSIE_TEMP_SPILL_BUDGET_BYTES", "1024")]);

        // Act
        let configured = CassieRuntimeConfig::from_env_reader(env_reader(current))
            .expect("current memory budget");
        let defaulted = CassieRuntimeConfig::from_env_reader(env_reader(obsolete))
            .expect("obsolete name should be ignored");

        // Assert
        assert_eq!(configured.limits.query_memory_budget_bytes, 4096);
        assert_eq!(
            defaulted.limits.query_memory_budget_bytes,
            CassieRuntimeLimits::default().query_memory_budget_bytes
        );
    }

    #[test]
    fn should_parse_admin_ui_dir() {
        // Arrange
        let values = HashMap::from([("CASSIE_ADMIN_UI_DIR", "/app/ui/dist".to_string())]);

        // Act
        let config =
            CassieRuntimeConfig::from_env_reader(env_reader_owned(values)).expect("runtime config");

        // Assert
        assert_eq!(config.admin_ui_dir, "/app/ui/dist");
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

    #[test]
    fn should_parse_voyage_embedding_runtime_config() {
        // Arrange
        let values = HashMap::from([
            ("CASSIE_VOYAGE_API_KEY", "voyage-secret"),
            ("CASSIE_VOYAGE_MODEL", "voyage-3.5"),
            ("CASSIE_VOYAGE_DIMENSIONS", "512"),
            ("CASSIE_VOYAGE_TIMEOUT_SECONDS", "9"),
            ("CASSIE_VOYAGE_MAX_BATCH_SIZE", "7"),
            ("CASSIE_VOYAGE_MAX_RETRIES", "2"),
            ("CASSIE_VOYAGE_BASE_URL", "https://voyage.example"),
        ]);

        // Act
        let config = parse_provider_config_from("voyage", env_reader(values));

        // Assert
        match config {
            EmbeddingsRuntimeConfig::Voyage(runtime) => {
                assert_eq!(runtime.api_key, "voyage-secret");
                assert_eq!(runtime.model, "voyage-3.5");
                assert_eq!(runtime.dimensions, 512);
                assert_eq!(runtime.timeout_seconds, 9);
                assert_eq!(runtime.max_batch_size, 7);
                assert_eq!(runtime.max_retries, 2);
                assert_eq!(runtime.base_url, "https://voyage.example");
            }
            _ => panic!("expected voyage config"),
        }
    }

    #[test]
    fn should_parse_cohere_embedding_runtime_config() {
        // Arrange
        let values = HashMap::from([
            ("CASSIE_COHERE_API_KEY", "cohere-secret"),
            ("CASSIE_COHERE_MODEL", "embed-v4.0"),
            ("CASSIE_COHERE_DIMENSIONS", "1024"),
            ("CASSIE_COHERE_TIMEOUT_SECONDS", "11"),
            ("CASSIE_COHERE_MAX_BATCH_SIZE", "14"),
            ("CASSIE_COHERE_MAX_RETRIES", "5"),
            ("CASSIE_COHERE_BASE_URL", "https://cohere.example"),
        ]);

        // Act
        let config = parse_provider_config_from("cohere", env_reader(values));

        // Assert
        match config {
            EmbeddingsRuntimeConfig::Cohere(runtime) => {
                assert_eq!(runtime.api_key, "cohere-secret");
                assert_eq!(runtime.model, "embed-v4.0");
                assert_eq!(runtime.dimensions, 1024);
                assert_eq!(runtime.timeout_seconds, 11);
                assert_eq!(runtime.max_batch_size, 14);
                assert_eq!(runtime.max_retries, 5);
                assert_eq!(runtime.base_url, "https://cohere.example");
            }
            _ => panic!("expected cohere config"),
        }
    }

    #[test]
    fn should_parse_local_embedding_runtime_config() {
        // Arrange
        let values = HashMap::from([
            ("CASSIE_LOCAL_MODEL", "cassie-local-hash-v2"),
            ("CASSIE_LOCAL_DIMENSIONS", "128"),
        ]);

        // Act
        let config = parse_provider_config_from("local", env_reader(values));

        // Assert
        match config {
            EmbeddingsRuntimeConfig::Local(runtime) => {
                assert_eq!(runtime.model, "cassie-local-hash-v2");
                assert_eq!(runtime.dimensions, 128);
            }
            _ => panic!("expected local config"),
        }
    }
}
