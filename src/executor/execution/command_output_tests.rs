//! Required command payload admission precedes the command's own mutation.
use std::path::PathBuf;
use std::time::Instant;

use crate::app::{Cassie, CassieError, CassieSession};
use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits};
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{QueryStatement, SetStatement};

use super::super::QueryError;

fn exhausted_controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 0,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("strict command-admission cleanup");
    }
}

#[test]
fn should_admit_command_output_before_creating_a_table() {
    // Arrange
    let directory = Directory(
        std::env::temp_dir().join(format!("cassie-command-admission-{}", uuid::Uuid::new_v4())),
    );
    let cassie = Cassie::new_with_data_dir_and_config(&directory.0, CassieRuntimeConfig::default())
        .expect("isolated local store");
    let parsed = crate::sql::parse_statement("CREATE TABLE denied_output(id BIGINT)")
        .expect("valid table statement");
    let QueryStatement::CreateTable(statement) = parsed.statement else {
        panic!("expected table statement");
    };
    let controls = exhausted_controls();

    // Act
    let result = super::super::schema_creation::create_table(&cassie, &statement, &controls);
    let catalog_created = cassie.catalog.exists("denied_output");
    let storage_created = cassie.midge.collection_schema("denied_output").is_some();
    drop(cassie);
    let restarted =
        Cassie::new_with_data_dir_and_config(&directory.0, CassieRuntimeConfig::default())
            .expect("fresh local-store owner");
    restarted.startup().expect("cold catalog recovery");
    let recovered = restarted.catalog.exists("denied_output");
    drop(restarted);
    drop(directory);

    // Assert
    let Err(QueryError::Cassie(error @ CassieError::ResourceLimit(_))) = result else {
        panic!("required command output must return structured memory denial");
    };
    assert_eq!(error.descriptor().sql_state, "54000");
    assert!(
        !catalog_created,
        "denied output must precede catalog publication"
    );
    assert!(
        !storage_created,
        "denied output must precede durable table creation"
    );
    assert!(
        !recovered,
        "cold recovery must not find a table from a denied command"
    );
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_command_output_before_changing_session_settings() {
    // Arrange
    let session = CassieSession::new("root".to_owned(), None);
    session
        .set_setting("application_name", "original")
        .expect("initial setting");
    let statement = SetStatement {
        variable: "application_name".to_owned(),
        value: Some("denied replacement".to_owned()),
    };
    let controls = exhausted_controls();

    // Act
    let result = super::super::session_command::execute_set(Some(&session), &statement, &controls);
    let actual = session
        .setting("application_name")
        .expect("setting readback");

    // Assert
    let Err(QueryError::Cassie(error @ CassieError::ResourceLimit(_))) = result else {
        panic!("required command output must return structured memory denial");
    };
    assert_eq!(error.descriptor().sql_state, "54000");
    assert_eq!(actual, "original");
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
