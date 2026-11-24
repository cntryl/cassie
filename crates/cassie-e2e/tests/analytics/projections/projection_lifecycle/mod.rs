#![allow(unused_imports, dead_code)]

use std::sync::{Arc, Barrier};

use cassie::app::{Cassie, CassieError, CassieSession, StorageRetryKind};
use cassie::app::{ProjectionReplayBatch, ProjectionReplayEvent};
use cassie::catalog::{ProjectionVerificationState, ProjectionVersionState};
use cassie::executor::set_projection_activation_failure_point;
use cassie::midge::adapter::{
    schema_write_conflict_test_guard, schema_write_conflict_worker_guard,
    set_projection_metadata_persistence_failure_point, set_projection_output_failure_point,
    set_projection_replay_metadata_commit_failure_point,
    set_projection_replay_post_commit_failure_point, set_projection_report_deletion_failure_point,
    set_schema_write_commit_barriers, ProjectionOutputFailurePoint, SchemaWritePausePoint,
};
use cassie::sql::ast::{
    AlterMaterializedProjectionOperation, CopyFormat, CopyStatement, QueryStatement,
};
use cassie::types::Value;

use super::support_sql as support;
use support::*;

fn execute_statement(cassie: &Cassie, session: &CassieSession, sql: &str) {
    cassie.execute_sql(session, sql, vec![]).unwrap();
}

fn query_rows(cassie: &Cassie, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(session, sql, vec![])
        .unwrap()
        .rows()
        .to_vec()
}

fn seed_materialized_projection_version_fixture(
    cassie: &Cassie,
    session: &CassieSession,
) -> String {
    execute_statement(
        cassie,
        session,
        "CREATE TABLE projection_version_docs (title TEXT)",
    );
    execute_statement(
        cassie,
        session,
        "INSERT INTO projection_version_docs (title) VALUES ('alpha')",
    );
    execute_statement(
        cassie,
        session,
        "CREATE MATERIALIZED PROJECTION projection_versioned AS SELECT title FROM projection_version_docs",
    );
    let projection = cassie
        .catalog
        .get_materialized_projection("projection_versioned")
        .expect("materialized projection metadata")
        .collection;
    execute_statement(
        cassie,
        session,
        "INSERT INTO projection_version_docs (title) VALUES ('bravo')",
    );
    projection
}

fn projection_version_rows(
    cassie: &Cassie,
    session: &CassieSession,
    projection: &str,
) -> Vec<Vec<Value>> {
    query_rows(
        cassie,
        session,
        &format!(
            "SELECT version_id, state FROM pg_catalog.pg_projection_versions WHERE projection_name = '{projection}' ORDER BY version_id"
        ),
    )
}

include!("semantics_parse_materialized_projection_lifecycle_commands.rs");

include!("metadata.rs");

include!("errors.rs");

fn start_projection_version_fixture(
    label: &str,
) -> (
    tokio::runtime::Runtime,
    String,
    Cassie,
    CassieSession,
    String,
) {
    start_projection_version_fixture_with_storage(label, false)
}

fn persistent_projection_version_fixture(
    label: &str,
) -> (
    tokio::runtime::Runtime,
    String,
    Cassie,
    CassieSession,
    String,
) {
    start_projection_version_fixture_with_storage(label, true)
}

fn start_projection_version_fixture_with_storage(
    label: &str,
    persistent: bool,
) -> (
    tokio::runtime::Runtime,
    String,
    Cassie,
    CassieSession,
    String,
) {
    let path = data_dir(label);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let cassie = runtime.block_on(async {
        let cassie = if persistent {
            Cassie::new_with_data_dir(&path).unwrap()
        } else {
            crate::support_sql::new_in_memory_cassie().unwrap()
        };
        cassie.startup().unwrap();
        cassie
    });
    let session = cassie.create_session("tester", None);
    let projection = seed_materialized_projection_version_fixture(&cassie, &session);
    (runtime, path, cassie, session, projection)
}

include!("semantics_remove_version_reports_when_dropping_projection_version.rs");

include!("restart.rs");
