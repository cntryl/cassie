//! Required transaction output is admitted before changing transaction state.
use std::path::PathBuf;
use std::time::Instant;

use super::*;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryExecutionControls;
use crate::types::Value;

struct Directory(PathBuf);

impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("strict transaction-admission cleanup");
    }
}

struct Fixture {
    cassie: Cassie,
    _directory: Directory,
}

impl Fixture {
    fn new() -> Self {
        let directory = Directory(std::env::temp_dir().join(format!(
            "cassie-transaction-output-{}",
            uuid::Uuid::new_v4()
        )));
        std::fs::create_dir_all(&directory.0).expect("owned fixture directory");
        let cassie = Cassie::new_with_data_dir(&directory.0).expect("isolated local store");
        cassie.startup().expect("startup");
        Self {
            cassie,
            _directory: directory,
        }
    }

    fn denied(
        &self,
        session: &CassieSession,
        action: TransactionAction,
    ) -> (Result<QueryResult, CassieError>, QueryExecutionControls) {
        let controls = QueryExecutionControls::from_limits(
            &CassieRuntimeLimits {
                query_memory_budget_bytes: 0,
                ..CassieRuntimeLimits::default()
            },
            Instant::now(),
        );
        let result = self.cassie.execute_transaction_statement(
            session,
            &TransactionStatement {
                action,
                isolation: None,
            },
            &controls,
        );
        (result, controls)
    }
}

fn assert_denied(result: Result<QueryResult, CassieError>, controls: &QueryExecutionControls) {
    let Err(error @ CassieError::ResourceLimit(_)) = result else {
        panic!("required transaction output must return structured memory denial");
    };
    assert_eq!(error.descriptor().sql_state, "54000");
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_transaction_output_before_beginning() {
    // Arrange
    let fixture = Fixture::new();
    let session = fixture.cassie.create_session("root", None);

    // Act
    let (result, controls) = fixture.denied(&session, TransactionAction::Begin);
    let status = session.transaction_status();
    session.rollback_transaction();
    drop((session, fixture));

    // Assert
    assert_denied(result, &controls);
    assert_eq!(status, "idle");
}

#[test]
fn should_admit_transaction_output_before_committing_staged_rows() {
    // Arrange
    let fixture = Fixture::new();
    let writer = fixture.cassie.create_session("root", None);
    let observer = fixture.cassie.create_session("root", None);
    let execute = |session: &CassieSession, sql: &str| {
        fixture
            .cassie
            .execute_sql(session, sql, vec![])
            .expect("SQL")
    };
    execute(&writer, "CREATE TABLE admission_rows(id BIGINT)");
    execute(&writer, "BEGIN");
    execute(&writer, "INSERT INTO admission_rows VALUES (1)");
    assert_eq!(
        execute(&writer, "SELECT id FROM admission_rows").rows(),
        &[vec![Value::Int64(1)]]
    );
    assert!(execute(&observer, "SELECT id FROM admission_rows")
        .rows()
        .is_empty());

    // Act
    let (result, controls) = fixture.denied(&writer, TransactionAction::Commit);
    let status = writer.transaction_status();
    let visible = execute(&observer, "SELECT id FROM admission_rows")
        .rows()
        .to_vec();
    writer.rollback_transaction();
    drop((writer, observer, fixture));

    // Assert
    assert_denied(result, &controls);
    assert_eq!(status, "in_transaction");
    assert_eq!(visible, Vec::<Vec<Value>>::new());
}

#[test]
fn should_admit_transaction_output_before_rolling_back() {
    // Arrange
    let fixture = Fixture::new();
    let session = fixture.cassie.create_session("root", None);
    session
        .set_setting("application_name", "before")
        .expect("initial setting");
    session.begin_transaction(None).expect("valid transaction");
    session
        .set_setting("application_name", "after")
        .expect("transaction setting");

    // Act
    let (result, controls) = fixture.denied(&session, TransactionAction::Rollback);
    let status = session.transaction_status();
    let setting = session
        .setting("application_name")
        .expect("setting readback");
    session.rollback_transaction();
    drop((session, fixture));

    // Assert
    assert_denied(result, &controls);
    assert_eq!(status, "in_transaction");
    assert_eq!(setting, "after");
}

#[test]
fn should_admit_transaction_output_before_creating_a_savepoint() {
    // Arrange
    let fixture = Fixture::new();
    let session = fixture.cassie.create_session("root", None);
    session.begin_transaction(None).expect("valid transaction");

    // Act
    let (result, controls) = fixture.denied(
        &session,
        TransactionAction::Savepoint {
            name: "denied".to_owned(),
        },
    );
    let created = session.release_savepoint("denied").is_ok();
    session.rollback_transaction();
    drop((session, fixture));

    // Assert
    assert_denied(result, &controls);
    assert!(!created, "denied command must not create a savepoint");
}

#[test]
fn should_admit_transaction_output_before_restoring_a_savepoint() {
    // Arrange
    let fixture = Fixture::new();
    let session = fixture.cassie.create_session("root", None);
    session.begin_transaction(None).expect("valid transaction");
    session
        .set_setting("application_name", "saved")
        .expect("saved setting");
    session.create_savepoint("kept").expect("valid savepoint");
    session
        .set_setting("application_name", "after")
        .expect("later setting");

    // Act
    let (result, controls) = fixture.denied(
        &session,
        TransactionAction::RollbackTo {
            name: "kept".to_owned(),
        },
    );
    let status = session.transaction_status();
    let setting = session
        .setting("application_name")
        .expect("setting readback");
    session.rollback_transaction();
    drop((session, fixture));

    // Assert
    assert_denied(result, &controls);
    assert_eq!(status, "in_transaction");
    assert_eq!(setting, "after");
}

#[test]
fn should_admit_transaction_output_before_releasing_a_savepoint() {
    // Arrange
    let fixture = Fixture::new();
    let session = fixture.cassie.create_session("root", None);
    session.begin_transaction(None).expect("valid transaction");
    session
        .set_setting("application_name", "saved")
        .expect("saved setting");
    session.create_savepoint("kept").expect("valid savepoint");
    session
        .set_setting("application_name", "after")
        .expect("later setting");

    // Act
    let (result, controls) = fixture.denied(
        &session,
        TransactionAction::Release {
            name: "kept".to_owned(),
        },
    );
    let retained = session.rollback_to_savepoint("kept").is_ok();
    let setting = session
        .setting("application_name")
        .expect("setting readback");
    session.rollback_transaction();
    drop((session, fixture));

    // Assert
    assert_denied(result, &controls);
    assert!(retained, "denied command must preserve its savepoint");
    assert_eq!(setting, "saved");
}
