use super::*;
use std::collections::HashMap;

fn env_reader(values: HashMap<&'static str, &'static str>) -> impl Fn(&str) -> Option<String> {
    move |key| values.get(key).map(std::string::ToString::to_string)
}

fn env_reader_owned(values: HashMap<&'static str, String>) -> impl Fn(&str) -> Option<String> {
    move |key| values.get(key).cloned()
}

#[test]
fn should_default_security_resource_limits_to_the_public_contract() {
    // Arrange
    // Act
    let config = CassieRuntimeConfig::default();

    // Assert
    assert_eq!(config.auth_user_attempts_per_minute, 10);
    assert_eq!(config.auth_ip_attempts_per_minute, 60);
    assert_eq!(config.auth_rate_limit_max_entries, 4_096);
    assert!(!config.rest_external_https);
    assert_eq!(config.embeddings_max_response_bytes, 8_388_608);
    assert_eq!(config.limits.rest_max_sessions_per_user, 16);
    assert_eq!(config.limits.rest_write_timeout_ms, 10_000);
    assert_eq!(config.limits.pgwire_write_timeout_ms, 10_000);
}

#[test]
fn should_not_supply_a_default_root_password_for_embedded_configuration() {
    // Arrange
    let values = HashMap::new();

    // Act
    let config = CassieRuntimeConfig::from_env_reader(env_reader_owned(values))
        .expect("embedded configuration without network credentials");

    // Assert
    assert_eq!(config.password, "");
}

#[test]
fn should_require_root_password_for_server_configuration() {
    // Arrange
    let values = HashMap::new();

    // Act
    let error = CassieRuntimeConfig::from_server_env_reader(env_reader_owned(values))
        .expect_err("server configuration must require a root password");

    // Assert
    assert!(matches!(
        error,
        CassieRuntimeConfigError::PasswordEnvironmentMissing {
            key: "CASSIE_ROOT_PASSWORD"
        }
    ));
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
fn should_reject_explicit_default_password_on_non_loopback_listener() {
    // Arrange
    let values = HashMap::from([
        ("CASSIE_PGWIRE_LISTEN", "0.0.0.0:5432"),
        ("CASSIE_ROOT_PASSWORD", "postgres"),
        ("CASSIE_ALLOW_INSECURE_NON_LOOPBACK_LISTEN", "1"),
    ]);

    // Act
    let error = CassieRuntimeConfig::from_server_env_reader(env_reader(values))
        .expect_err("the known default password must not expose a non-loopback listener");

    // Assert
    assert!(error.to_string().contains("unsafe"));
    assert!(error.to_string().contains("0.0.0.0:5432"));
}

#[test]
fn should_allow_secure_non_loopback_listener() {
    // Arrange
    let values = HashMap::from([
        ("CASSIE_PGWIRE_LISTEN", "0.0.0.0:5432"),
        ("CASSIE_ROOT_PASSWORD", "different-secret"),
        ("CASSIE_PGWIRE_TLS_CERT_FILE", "/etc/cassie/tls/cert.pem"),
        ("CASSIE_PGWIRE_TLS_KEY_FILE", "/etc/cassie/tls/key.pem"),
    ]);

    // Act
    let config = CassieRuntimeConfig::from_server_env_reader(env_reader(values))
        .expect("explicit password should permit non-loopback listener");

    // Assert
    assert_eq!(config.password, "different-secret");
}

#[test]
fn should_keep_the_root_administrative_identity_fixed() {
    // Arrange
    let values = HashMap::from([
        ("CASSIE_ADMIN_USER", "legacy-admin"),
        ("CASSIE_ROOT_PASSWORD", "root-secret"),
    ]);

    // Act
    let config =
        CassieRuntimeConfig::from_env_reader(env_reader(values)).expect("root configuration");
    let data_dir = std::env::temp_dir().join(format!("cassie-root-{}", uuid::Uuid::new_v4()));
    let cassie = crate::app::Cassie::new_with_data_dir_and_config(&data_dir, config)
        .expect("Cassie instance");

    // Assert
    assert!(cassie
        .authenticate_role("root", Some("root-secret"), None)
        .is_ok());
    assert!(cassie
        .authenticate_role("legacy-admin", Some("root-secret"), None)
        .is_err());
    drop(cassie);
    let _ = std::fs::remove_dir_all(data_dir);
}

#[test]
fn should_require_pgwire_tls_for_non_loopback_listener() {
    // Arrange
    let values = HashMap::from([
        ("CASSIE_PGWIRE_LISTEN", "0.0.0.0:5432"),
        ("CASSIE_ROOT_PASSWORD", "different-secret"),
    ]);

    // Act
    let error = CassieRuntimeConfig::from_env_reader(env_reader(values))
        .expect_err("non-loopback pgwire must require TLS");

    // Assert
    assert!(error.to_string().contains("pgwire TLS"));
    assert!(error.to_string().contains("0.0.0.0:5432"));
}

#[test]
fn should_allow_non_loopback_listeners_given_explicit_insecure_override() {
    // Arrange
    let values = HashMap::from([
        ("CASSIE_PGWIRE_LISTEN", "0.0.0.0:5432"),
        ("CASSIE_REST_LISTEN", "0.0.0.0:8080"),
        ("CASSIE_ROOT_PASSWORD", "different-secret"),
        ("CASSIE_ALLOW_INSECURE_NON_LOOPBACK_LISTEN", "1"),
    ]);

    // Act
    let config = CassieRuntimeConfig::from_env_reader(env_reader(values))
        .expect("explicit local-container override");

    // Assert
    assert!(config.allow_insecure_non_loopback_listen);
    assert!(config.pgwire_tls_cert_file.is_none());
    assert!(config.rest_tls_cert_file.is_none());
}

#[test]
fn should_reject_invalid_pgwire_listener() {
    // Arrange
    let values = HashMap::from([("CASSIE_PGWIRE_LISTEN", "localhost:5432")]);

    // Act
    let error = CassieRuntimeConfig::from_env_reader(env_reader(values))
        .expect_err("pgwire listener must be a literal socket address");

    // Assert
    assert!(matches!(
        error,
        CassieRuntimeConfigError::InvalidPgwireListener { listener }
            if listener == "localhost:5432"
    ));
}

#[test]
fn should_reject_invalid_rest_listener_with_insecure_override() {
    // Arrange
    let values = HashMap::from([
        ("CASSIE_REST_LISTEN", "localhost:8080"),
        ("CASSIE_ALLOW_INSECURE_NON_LOOPBACK_LISTEN", "1"),
    ]);

    // Act
    let error = CassieRuntimeConfig::from_env_reader(env_reader(values))
        .expect_err("REST listener must be parsed despite the insecure override");

    // Assert
    assert!(matches!(
        error,
        CassieRuntimeConfigError::InvalidRestListener { listener }
            if listener == "localhost:8080"
    ));
}

#[test]
fn should_reject_pgwire_when_only_a_tls_certificate_is_configured() {
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
fn should_reject_non_loopback_rest_without_tls() {
    // Arrange
    let values = HashMap::from([
        ("CASSIE_REST_LISTEN", "0.0.0.0:8080"),
        ("CASSIE_ROOT_PASSWORD", "different-secret"),
    ]);

    // Act
    let error = CassieRuntimeConfig::from_env_reader(env_reader(values))
        .expect_err("non-loopback REST must require TLS");

    // Assert
    assert!(error.to_string().contains("REST TLS"));
    assert!(error.to_string().contains("0.0.0.0:8080"));
}

#[test]
fn should_reject_rest_when_only_a_tls_key_is_configured() {
    // Arrange
    let values = HashMap::from([("CASSIE_REST_TLS_KEY_FILE", "/tmp/cassie-key.pem")]);

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
    let config = CassieRuntimeConfig::from_env_reader(env_reader(values)).expect("REST TLS paths");

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
    let config = CassieRuntimeConfig::from_env_reader(env_reader(values)).expect("runtime config");

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
    let configured =
        CassieRuntimeConfig::from_env_reader(env_reader(current)).expect("current memory budget");
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

#[test]
fn should_configure_history_memory_independently_of_query_memory() {
    // Arrange
    let configured = HashMap::from([
        ("CASSIE_QUERY_MEMORY_BUDGET_BYTES", "16"),
        ("CASSIE_REPEATABLE_READ_HISTORY_MEMORY_BUDGET_BYTES", "4096"),
    ]);
    let query_only = HashMap::from([("CASSIE_QUERY_MEMORY_BUDGET_BYTES", "16")]);

    // Act
    let explicit =
        CassieRuntimeConfig::from_env_reader(env_reader(configured)).expect("independent limits");
    let defaulted = CassieRuntimeConfig::from_env_reader(env_reader(query_only))
        .expect("default history limit");

    // Assert
    assert_eq!(explicit.limits.query_memory_budget_bytes, 16);
    assert_eq!(
        explicit.limits.repeatable_read_history_memory_budget_bytes,
        4096
    );
    assert_eq!(defaulted.limits.query_memory_budget_bytes, 16);
    assert_eq!(
        defaulted.limits.repeatable_read_history_memory_budget_bytes,
        10 * 1024 * 1024
    );
}
