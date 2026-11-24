//! Transaction-generation witnesses for COPY predicate admission.

use super::{bind_copy_from_stdin, execute, filter, request, BoundCopyRequest};
use crate::app::Cassie;
use crate::config::CassieRuntimeConfig;
use crate::types::Value;

#[test]
fn should_reject_stale_copy_owner_before_predicate_sequence_effect() {
    // Arrange
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE copy_stale_predicate (id INT)",
        "CREATE SEQUENCE copy_stale_predicate_seq",
        "BEGIN",
    ] {
        cassie.execute_sql(&session, sql, vec![]).expect("setup");
    }
    let request = request::parse_copy_request(
        "COPY copy_stale_predicate (id) FROM STDIN WHERE (id > 0 AND nextval('copy_stale_predicate_seq') > 0)",
    )
    .expect("parse COPY")
    .expect("COPY request");
    let bound = bind_copy_from_stdin(&cassie, &session, request, None).expect("bind COPY");
    let BoundCopyRequest::From(request, statement, binding) = bound else {
        panic!("COPY FROM binding")
    };
    cassie
        .execute_sql(&session, "ROLLBACK", vec![])
        .expect("retire owner");
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("new generation");

    // Act
    let controls = crate::midge::adapter::PublicationControlScope::enter(Some(&binding.controls));
    let result = execute::execute_copy_payload(
        &binding.cassie,
        &session,
        &request,
        &statement,
        b"41\n",
        None,
        binding.owner.as_ref(),
    );
    drop(controls);
    cassie
        .execute_sql(&session, "ROLLBACK", vec![])
        .expect("rollback");
    let next = cassie
        .execute_sql(
            &session,
            "SELECT nextval('copy_stale_predicate_seq')",
            vec![],
        )
        .expect("sequence");

    // Assert
    assert!(matches!(
        result,
        Err(filter::CopyWhereError::Cassie(crate::app::CassieError::Transaction {
            kind: crate::app::TransactionErrorKind::SerializationFailure,
            message,
        })) if message == "COPY owner no longer matches the active transaction"
    ));
    assert_eq!(next.rows(), &[vec![Value::Int64(1)]]);
}

#[test]
fn should_execute_sequence_predicate_with_current_copy_owner() {
    // Arrange
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE copy_stale_predicate (id INT)",
        "CREATE SEQUENCE copy_stale_predicate_seq",
        "BEGIN",
    ] {
        cassie.execute_sql(&session, sql, vec![]).expect("setup");
    }
    let request = request::parse_copy_request(
        "COPY copy_stale_predicate (id) FROM STDIN WHERE (id > 0 AND nextval('copy_stale_predicate_seq') > 0)",
    )
    .expect("parse COPY")
    .expect("COPY request");
    let bound = bind_copy_from_stdin(&cassie, &session, request, None).expect("bind COPY");
    let BoundCopyRequest::From(request, statement, binding) = bound else {
        panic!("COPY FROM binding")
    };

    // Act
    let controls = crate::midge::adapter::PublicationControlScope::enter(Some(&binding.controls));
    let result = execute::execute_copy_payload(
        &binding.cassie,
        &session,
        &request,
        &statement,
        b"41\n",
        None,
        binding.owner.as_ref(),
    );
    drop(controls);
    cassie
        .execute_sql(&session, "COMMIT", vec![])
        .expect("commit");
    let current = cassie
        .execute_sql(
            &session,
            "SELECT currval('copy_stale_predicate_seq')",
            vec![],
        )
        .expect("sequence");
    let rows = cassie
        .execute_sql(&session, "SELECT id FROM copy_stale_predicate", vec![])
        .expect("copied row");

    // Assert
    assert!(result.is_ok());
    assert_eq!(current.rows(), &[vec![Value::Int64(1)]]);
    assert_eq!(rows.rows(), &[vec![Value::Int64(41)]]);
}

#[test]
fn should_binary_copy_a_staged_enum_before_transaction_commit() {
    // Arrange
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    for sql in [
        "BEGIN",
        "CREATE TYPE copy_binary_mood AS ENUM ('sad', 'ok')",
        "CREATE TABLE copy_binary_enum (mood copy_binary_mood)",
    ] {
        cassie.execute_sql(&session, sql, Vec::new()).expect("setup SQL");
    }
    let request = request::parse_copy_request(
        "COPY copy_binary_enum (mood) FROM STDIN WITH (FORMAT BINARY)",
    )
    .expect("parse binary COPY")
    .expect("COPY request");
    let binding_sql = request.table_binding_sql().expect("table target");
    let operation = crate::app::transport::copy::CopyFromOperation::bind(
        std::sync::Arc::new(cassie.clone()),
        &session,
        &binding_sql,
        &crate::app::transport::copy::CopyForceSelection::None,
        &crate::app::transport::copy::CopyForceSelection::None,
        None,
    )
    .expect("bind staged enum COPY");
    let mut payload = b"PGCOPY\n\xff\r\n\0".to_vec();
    payload.extend_from_slice(&0_i32.to_be_bytes());
    payload.extend_from_slice(&0_i32.to_be_bytes());
    payload.extend_from_slice(&1_i16.to_be_bytes());
    payload.extend_from_slice(&2_i32.to_be_bytes());
    payload.extend_from_slice(b"ok");
    payload.extend_from_slice(&(-1_i16).to_be_bytes());

    // Act
    let result = operation.with_publication_scope(|| {
        execute::execute_decoded_operation(&session, &request, &operation, &payload, None)
    });
    let selected = cassie
        .execute_sql(&session, "SELECT mood FROM copy_binary_enum", Vec::new())
        .expect("read staged enum value");

    // Assert
    assert!(result.is_ok());
    assert!(matches!(
        selected.rows().first().and_then(|row| row.first()),
        Some(crate::types::Value::Typed(crate::types::typed::TypedValue::Enum(value)))
            if value.label() == "ok"
    ));
    cassie
        .execute_sql(&session, "ROLLBACK", Vec::new())
        .expect("rollback");
}

struct EntryFixture {
    cassie: Cassie,
    session: crate::app::CassieSession,
    request: request::CopyRequest,
    statement: Box<crate::sql::ast::CopyStatement>,
    binding: Box<super::CopyFromBinding>,
}

impl EntryFixture {
    fn new(sql: &str, cancellation: Option<crate::runtime::QueryCancellationHandle>) -> Self {
        Self::new_with_savepoint(sql, cancellation, false)
    }

    fn new_with_savepoint(
        sql: &str,
        cancellation: Option<crate::runtime::QueryCancellationHandle>,
        savepoint: bool,
    ) -> Self {
        let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default())
            .expect("memory fixture");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        for setup in [
            "CREATE TABLE copy_entry_precedence (id INT)",
            "CREATE SEQUENCE copy_entry_precedence_seq",
            "BEGIN",
        ] {
            cassie.execute_sql(&session, setup, vec![]).expect("setup");
        }
        if savepoint {
            cassie
                .execute_sql(&session, "SAVEPOINT copy_entry", vec![])
                .expect("savepoint");
        }
        let request = request::parse_copy_request(sql)
            .expect("parse COPY")
            .expect("COPY request");
        let bound =
            bind_copy_from_stdin(&cassie, &session, request, cancellation).expect("bind COPY");
        let BoundCopyRequest::From(request, statement, binding) = bound else {
            panic!("COPY FROM binding")
        };
        Self {
            cassie,
            session,
            request,
            statement,
            binding,
        }
    }

    fn run(
        &self,
        session: &crate::app::CassieSession,
        payload: &[u8],
    ) -> Result<execute::CopyCompletion, filter::CopyWhereError> {
        let _controls =
            crate::midge::adapter::PublicationControlScope::enter(Some(&self.binding.controls));
        execute::execute_copy_payload(
            &self.binding.cassie,
            session,
            &self.request,
            &self.statement,
            payload,
            None,
            self.binding.owner.as_ref(),
        )
    }

    fn retire(&self) {
        self.cassie
            .execute_sql(&self.session, "ROLLBACK", vec![])
            .expect("retire owner");
    }
}

const ENTRY_COPY_SQL: &str = "COPY copy_entry_precedence (id) FROM STDIN WHERE (id > 0 AND nextval('copy_entry_precedence_seq') > 0)";

#[test]
fn should_preserve_failed_transaction_error_for_current_copy_owner() {
    // Arrange
    let fixture = EntryFixture::new(ENTRY_COPY_SQL, None);
    fixture.session.mark_transaction_failed();

    // Act
    let result = fixture.run(&fixture.session, b"41\n");

    // Assert
    assert!(matches!(
        result,
        Err(filter::CopyWhereError::Cassie(
            crate::app::CassieError::Transaction {
                kind: crate::app::TransactionErrorKind::FailedTransaction,
                ..
            }
        ))
    ));
}

#[test]
fn should_preserve_cancellation_before_rejecting_stale_copy_owner() {
    // Arrange
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let fixture = EntryFixture::new(ENTRY_COPY_SQL, Some(cancellation.clone()));
    fixture.retire();
    cancellation.cancel();

    // Act
    let result = fixture.run(&fixture.session, b"41\n");

    // Assert
    assert!(matches!(
        result,
        Err(filter::CopyWhereError::Cassie(
            crate::app::CassieError::QueryCanceled
        ))
    ));
}

#[test]
fn should_preserve_deadline_before_rejecting_stale_copy_owner() {
    // Arrange
    let mut fixture = EntryFixture::new(ENTRY_COPY_SQL, None);
    fixture.retire();
    fixture.binding.controls.deadline = Some(std::time::Instant::now());

    // Act
    let result = fixture.run(&fixture.session, b"41\n");

    // Assert
    assert!(matches!(
        result,
        Err(filter::CopyWhereError::Cassie(
            crate::app::CassieError::DeadlineExceeded
        ))
    ));
}

#[test]
fn should_preserve_invalid_header_diagnostic_for_stale_copy_owner() {
    // Arrange
    let fixture = EntryFixture::new(
        "COPY copy_entry_precedence (id) FROM STDIN WITH (FORMAT CSV, HEADER MATCH) WHERE (id > 0)",
        None,
    );
    fixture.retire();

    // Act
    let result = fixture.run(&fixture.session, b"wrong\n41\n");

    // Assert
    assert!(matches!(
        result,
        Err(filter::CopyWhereError::Cassie(error)) if error.sql_state() == "22P04"
    ));
}

#[test]
fn should_reject_copy_owner_from_another_active_session() {
    // Arrange
    let fixture = EntryFixture::new(ENTRY_COPY_SQL, None);
    let other = fixture.cassie.create_session("other", None);
    fixture
        .cassie
        .execute_sql(&other, "BEGIN", vec![])
        .expect("other transaction");

    // Act
    let result = fixture.run(&other, b"41\n");

    // Assert
    assert!(matches!(
        result,
        Err(filter::CopyWhereError::Cassie(crate::app::CassieError::Transaction {
            kind: crate::app::TransactionErrorKind::SerializationFailure,
            message,
        })) if message == "COPY owner no longer matches the active transaction"
    ));
}

#[test]
fn should_preserve_autocommit_copy_without_owner() {
    // Arrange
    let fixture = EntryFixture::new(ENTRY_COPY_SQL, None);
    fixture.retire();
    let bound = bind_copy_from_stdin(&fixture.cassie, &fixture.session, fixture.request, None)
        .expect("autocommit binding");
    let BoundCopyRequest::From(request, statement, binding) = bound else {
        panic!("COPY FROM binding")
    };
    assert!(binding.owner.is_none());

    // Act
    let controls = crate::midge::adapter::PublicationControlScope::enter(Some(&binding.controls));
    let result = execute::execute_copy_payload(
        &binding.cassie,
        &fixture.session,
        &request,
        &statement,
        b"41\n",
        None,
        binding.owner.as_ref(),
    );
    drop(controls);
    let rows = fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "SELECT id FROM copy_entry_precedence",
            vec![],
        )
        .expect("read row");

    // Assert
    assert!(result.is_ok());
    assert_eq!(rows.rows(), &[vec![Value::Int64(41)]]);
}

#[test]
fn should_reject_copy_owner_when_session_database_changes() {
    // Arrange
    let fixture = EntryFixture::new(ENTRY_COPY_SQL, None);
    let mut other_database = fixture.session.clone();
    other_database.database = Some("other_database".to_owned());

    // Act
    let result = fixture.run(&other_database, b"41\n");

    // Assert
    assert!(matches!(
        result,
        Err(filter::CopyWhereError::Cassie(crate::app::CassieError::Transaction {
            kind: crate::app::TransactionErrorKind::SerializationFailure,
            message,
        })) if message == "COPY owner no longer matches the active transaction"
    ));
}

#[test]
fn should_reject_copy_predicate_after_savepoint_restoration() {
    // Arrange
    let fixture = EntryFixture::new_with_savepoint(ENTRY_COPY_SQL, None, true);
    let transaction = fixture.session.clone();
    transaction
        .rollback_to_savepoint("copy_entry")
        .expect("restore unchanged savepoint");

    // Act
    let result = fixture.run(&fixture.session, b"41\n");
    fixture.retire();
    let next = fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "SELECT nextval('copy_entry_precedence_seq')",
            vec![],
        )
        .expect("sequence");
    let rows = fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "SELECT id FROM copy_entry_precedence",
            vec![],
        )
        .expect("rows");

    // Assert
    assert!(matches!(
        result,
        Err(filter::CopyWhereError::Cassie(crate::app::CassieError::Transaction {
            kind: crate::app::TransactionErrorKind::SerializationFailure,
            message,
        })) if message == "COPY owner no longer matches the active transaction"
    ));
    assert_eq!(next.rows(), &[vec![Value::Int64(1)]]);
    assert_eq!(rows.rows(), &[] as &[Vec<Value>]);
}

#[test]
fn should_execute_fresh_copy_predicate_after_savepoint_restoration() {
    // Arrange
    let fixture = EntryFixture::new_with_savepoint(ENTRY_COPY_SQL, None, true);
    fixture
        .session
        .rollback_to_savepoint("copy_entry")
        .expect("restore savepoint");
    let bound = bind_copy_from_stdin(
        &fixture.cassie,
        &fixture.session,
        fixture.request.clone(),
        None,
    )
    .expect("fresh binding");
    let BoundCopyRequest::From(request, statement, binding) = bound else {
        panic!("COPY FROM binding")
    };

    // Act
    let result = {
        let _controls =
            crate::midge::adapter::PublicationControlScope::enter(Some(&binding.controls));
        execute::execute_copy_payload(
            &binding.cassie,
            &fixture.session,
            &request,
            &statement,
            b"41\n",
            None,
            binding.owner.as_ref(),
        )
    };
    fixture
        .cassie
        .execute_sql(&fixture.session, "COMMIT", vec![])
        .expect("commit");
    let current = fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "SELECT currval('copy_entry_precedence_seq')",
            vec![],
        )
        .expect("sequence");
    let rows = fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "SELECT id FROM copy_entry_precedence",
            vec![],
        )
        .expect("rows");

    // Assert
    assert!(matches!(result, Ok(ref completion) if completion.command == "COPY 1"));
    assert_eq!(current.rows(), &[vec![Value::Int64(1)]]);
    assert_eq!(rows.rows(), &[vec![Value::Int64(41)]]);
}

#[test]
fn should_report_stale_entry_before_failed_transaction_after_savepoint_restore() {
    // Arrange
    let fixture = EntryFixture::new_with_savepoint(ENTRY_COPY_SQL, None, true);
    fixture
        .session
        .rollback_to_savepoint("copy_entry")
        .expect("rollback");
    fixture.session.mark_transaction_failed();

    // Act
    let result = fixture.run(&fixture.session, b"41\n");

    // Assert
    assert!(matches!(result, Err(filter::CopyWhereError::Cassie(
        crate::app::CassieError::Transaction {
            kind: crate::app::TransactionErrorKind::SerializationFailure,
            message,
        })) if message == "COPY owner no longer matches the active transaction"));
}

#[test]
fn should_preserve_header_error_before_savepoint_stale_owner() {
    // Arrange
    let fixture = EntryFixture::new_with_savepoint(
        "COPY copy_entry_precedence (id) FROM STDIN WITH (FORMAT CSV, HEADER MATCH) WHERE (id > 0)",
        None,
        true,
    );
    fixture
        .session
        .rollback_to_savepoint("copy_entry")
        .expect("rollback");

    // Act
    let result = fixture.run(&fixture.session, b"wrong\n41\n");

    // Assert
    assert!(
        matches!(result, Err(filter::CopyWhereError::Cassie(error)) if error.sql_state() == "22P04")
    );
}

#[test]
fn should_reject_copy_binding_across_savepoint_restore() {
    // Arrange
    let fixture = EntryFixture::new_with_savepoint(ENTRY_COPY_SQL, None, true);
    let session = fixture.session.clone();

    // Act: the callback runs after the provisional owner resolved this target.
    let stale = fixture.cassie.capture_copy_command_owner_after_provisional(
        &fixture.session,
        "public.copy_entry_precedence",
        &fixture.binding.controls,
        || session.rollback_to_savepoint("copy_entry"),
    );
    let first = fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "SELECT nextval('copy_entry_precedence_seq')",
            vec![],
        )
        .expect("unadvanced sequence");
    let before = fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "SELECT id FROM copy_entry_precedence",
            vec![],
        )
        .expect("rows before fresh bind");
    let bound = bind_copy_from_stdin(
        &fixture.cassie,
        &fixture.session,
        fixture.request.clone(),
        None,
    )
    .expect("fresh bind");
    let BoundCopyRequest::From(request, statement, binding) = bound else {
        panic!("COPY FROM binding")
    };
    let fresh = {
        let _controls =
            crate::midge::adapter::PublicationControlScope::enter(Some(&binding.controls));
        execute::execute_copy_payload(
            &binding.cassie,
            &fixture.session,
            &request,
            &statement,
            b"41\n",
            None,
            binding.owner.as_ref(),
        )
    };
    fixture
        .cassie
        .execute_sql(&fixture.session, "COMMIT", vec![])
        .expect("commit");
    let current = fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "SELECT currval('copy_entry_precedence_seq')",
            vec![],
        )
        .expect("sequence");
    let rows = fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "SELECT id FROM copy_entry_precedence",
            vec![],
        )
        .expect("rows");

    // Assert
    assert!(matches!(stale, Err(crate::app::CassieError::Transaction {
        kind: crate::app::TransactionErrorKind::SerializationFailure,
        message,
    }) if message == "COPY owner no longer matches the active transaction"));
    assert_eq!(first.rows(), &[vec![Value::Int64(1)]]);
    assert_eq!(before.rows(), &[] as &[Vec<Value>]);
    assert!(matches!(fresh, Ok(ref completion) if completion.command == "COPY 1"));
    assert_eq!(current.rows(), &[vec![Value::Int64(2)]]);
    assert_eq!(rows.rows(), &[vec![Value::Int64(41)]]);
}

#[test]
fn should_preserve_cancellation_before_savepoint_stale_owner() {
    // Arrange
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let fixture =
        EntryFixture::new_with_savepoint(ENTRY_COPY_SQL, Some(cancellation.clone()), true);
    fixture
        .session
        .rollback_to_savepoint("copy_entry")
        .expect("rollback");
    cancellation.cancel();

    // Act
    let result = fixture.run(&fixture.session, b"41\n");

    // Assert
    assert!(matches!(
        result,
        Err(filter::CopyWhereError::Cassie(
            crate::app::CassieError::QueryCanceled
        ))
    ));
}

#[test]
fn should_preserve_deadline_before_savepoint_stale_owner() {
    // Arrange
    let mut fixture = EntryFixture::new_with_savepoint(ENTRY_COPY_SQL, None, true);
    fixture
        .session
        .rollback_to_savepoint("copy_entry")
        .expect("rollback");
    fixture.binding.controls.deadline = Some(std::time::Instant::now());

    // Act
    let result = fixture.run(&fixture.session, b"41\n");

    // Assert
    assert!(matches!(
        result,
        Err(filter::CopyWhereError::Cassie(
            crate::app::CassieError::DeadlineExceeded
        ))
    ));
}
