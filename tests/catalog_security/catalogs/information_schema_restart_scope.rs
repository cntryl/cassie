//! Stable information-schema isolation through a persisted rename and drop.

use cassie::app::{Cassie, CassieSession};
use cassie::config::CassieRuntimeConfig;
use cassie::types::Value;

const CHILD_MARKER: &str = "CASSIE_TEST_INFORMATION_SCHEMA_RESTART_CHILD";
const CHILD_COMPLETION: &str = "information-schema-restart-child-complete";
const TEST_NAME: &str = "information_schema_restart_scope::should_preserve_database_scoped_information_schema_after_drop_restart";

fn enter_local_storage_child() -> bool {
    if std::env::var_os(CHILD_MARKER).is_some() {
        assert_eq!(std::env::var("CASSIE_STORAGE_MODE").as_deref(), Ok("local"));
        return true;
    }

    let mut command = std::process::Command::new(std::env::current_exe().expect("test executable"));
    command.args(["--exact", TEST_NAME, "--nocapture", "--test-threads=1"]);
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("CASSIE_STORAGE_") {
            command.env_remove(key);
        }
    }
    let output = command
        .env(CHILD_MARKER, "1")
        .env("CASSIE_STORAGE_MODE", "local")
        .output()
        .expect("run isolated local-storage test child");
    assert!(
        output.status.success(),
        "isolated local-storage test child failed: {}",
        output.status
    );
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .any(|line| line == CHILD_COMPLETION),
        "the selected child must execute all lifecycle assertions"
    );
    false
}

fn execute(cassie: &Cassie, session: &CassieSession, sql: &str) {
    cassie
        .execute_sql(session, sql, vec![])
        .unwrap_or_else(|error| panic!("statement failed: {sql}: {error}"));
}

fn rows(cassie: &Cassie, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(session, sql, vec![])
        .unwrap_or_else(|error| panic!("query failed: {sql}: {error}"))
        .rows()
        .to_vec()
}

const TABLES: &str = "SELECT table_name, table_type FROM information_schema.tables \
    WHERE table_name IN ('catalog_scope_items', 'catalog_scope_renamed') ORDER BY table_name";
const COLUMNS: &str = "SELECT table_name, column_name, ordinal_position, is_nullable, \
    data_type, column_default FROM information_schema.columns \
    WHERE table_name IN ('catalog_scope_items', 'catalog_scope_renamed') \
    ORDER BY table_name, ordinal_position";

fn postgres_columns() -> Vec<Vec<Value>> {
    vec![
        vec![
            Value::String("catalog_scope_items".into()),
            Value::String("id".into()),
            Value::Int64(1),
            Value::String("NO".into()),
            Value::String("integer".into()),
            Value::String("7".into()),
        ],
        vec![
            Value::String("catalog_scope_items".into()),
            Value::String("note".into()),
            Value::Int64(2),
            Value::String("YES".into()),
            Value::String("text".into()),
            Value::Null,
        ],
    ]
}

fn tenant_columns() -> Vec<Vec<Value>> {
    vec![
        vec![
            Value::String("catalog_scope_items".into()),
            Value::String("email".into()),
            Value::Int64(1),
            Value::String("NO".into()),
            Value::String("text".into()),
            Value::String("'tenant'".into()),
        ],
        vec![
            Value::String("catalog_scope_items".into()),
            Value::String("quantity".into()),
            Value::Int64(2),
            Value::String("YES".into()),
            Value::String("integer".into()),
            Value::Null,
        ],
    ]
}

#[test]
fn should_preserve_database_scoped_information_schema_after_drop_restart() {
    // Arrange
    if !enter_local_storage_child() {
        return;
    }
    let directory =
        crate::support_catalog_directory::CatalogDirectory::new("information-schema-drop-restart");
    let postgres_columns = postgres_columns();
    let tenant_columns = tenant_columns();
    let table = vec![vec![
        Value::String("catalog_scope_items".into()),
        Value::String("BASE TABLE".into()),
    ]];

    // Act
    {
        let cassie =
            Cassie::new_with_data_dir_and_config(&directory.0, CassieRuntimeConfig::default())
                .expect("create first local instance");
        cassie.startup().expect("start first local instance");
        let postgres = cassie.create_session("tester", Some("postgres".into()));
        execute(&cassie, &postgres, "CREATE DATABASE catalog_scope_tenant");
        execute(
            &cassie,
            &postgres,
            "CREATE TABLE catalog_scope_items (id INT NOT NULL DEFAULT 7, note TEXT)",
        );
        let tenant = cassie.create_session("tester", Some("catalog_scope_tenant".into()));
        execute(
            &cassie,
            &tenant,
            "CREATE TABLE catalog_scope_items (email TEXT NOT NULL DEFAULT 'tenant', quantity INT)",
        );
        assert_eq!(rows(&cassie, &postgres, TABLES), table);
        assert_eq!(rows(&cassie, &tenant, TABLES), table);
        assert_eq!(rows(&cassie, &postgres, COLUMNS), postgres_columns);
        assert_eq!(rows(&cassie, &tenant, COLUMNS), tenant_columns);
        execute(
            &cassie,
            &postgres,
            "ALTER TABLE catalog_scope_items RENAME TO catalog_scope_renamed",
        );
        assert_eq!(
            rows(&cassie, &postgres, TABLES),
            vec![vec![
                Value::String("catalog_scope_renamed".into()),
                Value::String("BASE TABLE".into())
            ]]
        );
        assert_eq!(rows(&cassie, &tenant, TABLES), table);
        execute(&cassie, &postgres, "DROP TABLE catalog_scope_renamed");
        drop((postgres, tenant));
        cassie.shutdown();
    }

    let reopened =
        Cassie::new_with_data_dir_and_config(&directory.0, CassieRuntimeConfig::default())
            .expect("reopen local instance");
    reopened.startup().expect("start reopened instance");
    let postgres = reopened.create_session("tester", Some("postgres".into()));
    let tenant = reopened.create_session("tester", Some("catalog_scope_tenant".into()));
    let postgres_tables = rows(&reopened, &postgres, TABLES);
    let postgres_after = rows(&reopened, &postgres, COLUMNS);
    let tenant_tables = rows(&reopened, &tenant, TABLES);
    let tenant_after = rows(&reopened, &tenant, COLUMNS);

    // Assert
    assert!(
        postgres_tables.is_empty(),
        "dropped table stays absent after restart"
    );
    assert!(
        postgres_after.is_empty(),
        "dropped columns stay absent after restart"
    );
    assert_eq!(tenant_tables, table, "the tenant table remains visible");
    assert_eq!(
        tenant_after, tenant_columns,
        "the tenant columns remain truthful"
    );
    drop((postgres, tenant));
    reopened.shutdown();
    println!("\n{CHILD_COMPLETION}");
}
