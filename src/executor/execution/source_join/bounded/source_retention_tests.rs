use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use crate::app::Cassie;
use crate::catalog::canonical_relation_name;
use crate::config::CassieRuntimeConfig;
use crate::executor::batch::RowAccess;
use crate::runtime::QueryExecutionControls;
use crate::types::Value;

use super::{load_collection_rows, SourceExecutionEnv};

struct Fixture {
    cassie: Option<Cassie>,
    config: CassieRuntimeConfig,
    path: PathBuf,
    collection: String,
}

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "cassie-bounded-full-source-guard-{}",
            uuid::Uuid::new_v4()
        ));
        let mut config = CassieRuntimeConfig::default();
        config.limits.query_timeout_ms = 0;
        config.limits.query_memory_budget_bytes = 4 * 1024 * 1024;
        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone())
            .expect("retained full-source fixture");
        cassie.startup().expect("bootstrap full-source catalog");
        let session = cassie.create_session("root", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE bounded_full_source_guard (payload TEXT, arr INT[], extra JSON)",
                vec![],
            )
            .expect("full-source table");
        let collection = canonical_relation_name("postgres", "public", "bounded_full_source_guard");
        cassie
            .midge
            .put_document(
                &collection,
                Some("row-0".to_owned()),
                serde_json::json!({
                    "payload": "p".repeat(8192),
                    "arr": [1, 2],
                    "extra": {"retained": true},
                }),
            )
            .expect("full row with ARRAY metadata and JSON field");
        Self {
            cassie: Some(cassie),
            config,
            path,
            collection,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.cassie.take();
        std::fs::remove_dir_all(&self.path).expect("remove full-source fixture");
    }
}

#[test]
fn should_hold_full_source_body_admission_through_bounded_build_ownership() {
    // Arrange
    let fixture = Fixture::new();
    let cassie = fixture.cassie.as_ref().expect("live full-source fixture");
    let controls = QueryExecutionControls::from_limits(&fixture.config.limits, Instant::now());
    let functions = HashMap::new();
    let env = SourceExecutionEnv {
        cassie,
        session: None,
        user_functions: &functions,
        params: &[],
        controls: &controls,
    };

    // Act
    let rows = load_collection_rows(&env, &fixture.collection)
        .expect("the complete full source fits its configured budget");

    // Assert
    assert_eq!(rows.len(), 1);
    let row = &rows.as_slice()[0];
    let Value::String(payload) = row.get("payload").expect("full payload") else {
        panic!("TEXT payload");
    };
    assert_eq!(payload.len(), 8192);
    assert!(
        row.get("extra").is_some(),
        "full rows preserve the schema JSON field"
    );
    assert!(row.has_array_types(), "full rows preserve ARRAY metadata");
    assert!(
        controls.current_query_memory_bytes() >= payload.capacity(),
        "retained body requires at least {} TEXT bytes but only {} remain admitted",
        payload.capacity(),
        controls.current_query_memory_bytes()
    );
    assert!(
        row.query_memory().is_some(),
        "the original full body lease must survive qualification"
    );
    drop(rows);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
