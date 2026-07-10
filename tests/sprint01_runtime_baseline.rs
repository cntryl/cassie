use cassie::catalog::canonical_relation_name;
use cassie::config::EmbeddingsRuntimeConfig;
use cassie::types::{DataType, FieldSchema, Schema};
use cassie::{app::CassieError, Cassie, CassieRuntimeConfig};
use std::env;
use std::path::PathBuf;
use std::sync::Mutex;
use uuid::Uuid;

static CONFIG_ENV_LOCK: Mutex<()> = Mutex::new(());

fn without_fallback() {
    env::remove_var("CASSIE_MIDGE_ALLOW_FALLBACK");
}

fn data_dir(label: &str) -> String {
    let mut dir = env::temp_dir();
    dir.push(format!("cassie-v1-runtime-{}-{}", label, Uuid::new_v4()));
    dir.to_string_lossy().to_string()
}

struct EnvGuard {
    values: Vec<(&'static str, Option<String>)>,
}

impl EnvGuard {
    fn capture(keys: &[&'static str]) -> Self {
        Self {
            values: keys.iter().map(|key| (*key, env::var(key).ok())).collect(),
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, value) in &self.values {
            if let Some(value) = value {
                env::set_var(key, value);
            } else {
                env::remove_var(key);
            }
        }
    }
}

#[test]
fn should_startup_be_idempotent_without_state_corruption() {
    // Arrange
    without_fallback();
    let path = data_dir("idempotent_no_corruption");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();

        let collection = canonical_relation_name("postgres", "public", "runtime_docs");
        let schema = Schema {
            fields: vec![
                FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
                FieldSchema {
                    name: "body".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                },
            ],
        };

        cassie
            .midge
            .create_collection(&collection, schema.clone())
            .unwrap();
        let _ = cassie
            .midge
            .put_document(
                &collection,
                Some("doc-1".to_string()),
                serde_json::json!({"title": "alpha", "body": "first"}),
            )
            .unwrap();

        cassie.startup().unwrap();
        let baseline_collections = cassie.catalog.list_collections();
        let baseline_docs = cassie.midge.scan_documents(&collection).unwrap();
        let baseline_layout = cassie.midge.ensure_families_ready().unwrap().clone();

        // Act
        cassie.startup().unwrap();
        let after_collections = cassie.catalog.list_collections();
        let after_docs = cassie.midge.scan_documents(&collection).unwrap();
        let after_layout = cassie.midge.ensure_families_ready().unwrap().clone();

        // Assert
        assert_eq!(baseline_layout.schema.id(), after_layout.schema.id());
        assert_eq!(baseline_layout.data.id(), after_layout.data.id());
        assert_eq!(baseline_layout.temp.id(), after_layout.temp.id());
        assert_eq!(baseline_collections.len(), after_collections.len());
        assert_eq!(baseline_docs.len(), after_docs.len());

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_map_storage_bootstrap_failure_to_cassie_error() {
    // Arrange
    without_fallback();
    let base_path = PathBuf::from(data_dir("invalid_bootstrap"));
    let marker = base_path.join("marker");
    let _ = std::fs::create_dir_all(&base_path);
    let _ = std::fs::write(&marker, "locked");
    let path = format!("{}/child", marker.to_string_lossy());

    // Act
    let created = Cassie::new_with_data_dir(&path);

    // Assert
    assert!(matches!(
        created,
        Err(CassieError::Storage(_)
            | CassieError::StorageBootstrap(_)
            | CassieError::StorageMissingFamily(_)
            | CassieError::StorageRetryable(_))
    ));

    let _ = std::fs::remove_file(marker);
}

#[test]
fn should_startup_not_create_side_effects_in_default_family() {
    // Arrange
    let path = data_dir("default_family_side_effects");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();

        let collection = "runtime_default_guard";
        let schema = Schema {
            fields: vec![FieldSchema {
                name: "title".to_string(),
                data_type: DataType::Text,
                nullable: true,
            }],
        };

        cassie.midge.create_collection(collection, schema).unwrap();
        let _ = cassie
            .midge
            .put_document(
                collection,
                Some("doc-default-guard".to_string()),
                serde_json::json!({"title": "alpha"}),
            )
            .unwrap();
        cassie.startup().unwrap();

        // Act
        let default_entries = cassie.midge.raw_scan_prefix_named("default", b"").unwrap();

        // Assert
        for (key, _) in default_entries {
            let key = String::from_utf8_lossy(&key);
            assert!(
                !key.starts_with("__cassie__/"),
                "no cassie-managed keys should be stored in default family"
            );
        }

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_health_after_startup_reports_ready_state() {
    // Arrange
    let path = data_dir("startup_health");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();

        let before = cassie.health();
        assert_eq!(before["ready"].as_bool(), Some(false));

        cassie.startup().unwrap();

        // Act
        let after = cassie.health();

        // Assert
        assert_eq!(after["ready"].as_bool(), Some(true));
        assert_eq!(after["status"], "ok");

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_clear_ready_state_after_shutdown() {
    // Arrange
    let path = data_dir("shutdown_state");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();

        // Act
        cassie.shutdown();
        let after_health = cassie.health();
        let after_metrics = cassie.metrics();

        // Assert
        assert_eq!(after_health["ready"].as_bool(), Some(false));
        assert_eq!(after_health["status"], "starting");
        assert_eq!(after_metrics["runtime"]["started"].as_bool(), Some(false));
        assert_eq!(after_metrics["runtime"]["shutdown_total"].as_u64(), Some(1));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_startup_respects_runtime_config_defaults() {
    // Arrange
    let _env_lock = CONFIG_ENV_LOCK.lock().expect("config env lock");
    let keys = [
        "CASSIE_PGWIRE_LISTEN",
        "CASSIE_REST_LISTEN",
        "CASSIE_ADMIN_USER",
        "CASSIE_DEFAULT_DATABASE",
        "CASSIE_ADMIN_PASSWORD",
        "CASSIE_ADMIN_PASSWORD_FILE",
        "CASSIE_EMBEDDINGS_PROVIDER",
    ];
    let _guard = EnvGuard::capture(&keys);

    for key in keys {
        env::remove_var(key);
    }

    // Act
    let config = CassieRuntimeConfig::from_env().expect("runtime config");

    // Assert
    assert_eq!(config.pgwire_listen, "127.0.0.1:5432");
    assert_eq!(config.rest_listen, "127.0.0.1:8080");
    assert_eq!(config.user, "postgres");
    assert_eq!(config.password, "postgres");
    assert!(matches!(
        config.embeddings,
        EmbeddingsRuntimeConfig::Disabled
    ));
}

#[test]
fn should_create_session_without_mutating_runtime_state() {
    // Arrange
    let path = data_dir("session_immutability");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();

        let before_health = cassie.health();
        let before_collections = cassie.catalog.list_collections().len();

        // Act
        let session = cassie.create_session("tester", Some("postgres".to_string()));
        let after_health = cassie.health();
        let after_collections = cassie.catalog.list_collections().len();

        // Assert
        assert_eq!(session.user, "tester");
        assert_eq!(session.database, Some("postgres".to_string()));
        assert_eq!(
            before_health["ready"].as_bool(),
            after_health["ready"].as_bool()
        );
        assert_eq!(before_health["status"], after_health["status"]);
        assert_eq!(before_collections, after_collections);

        let _ = std::fs::remove_dir_all(path);
    });
}
