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
    let _controls = crate::midge::adapter::PublicationControlScope::enter(Some(&binding.controls));
    let result = execute::execute_copy_payload(
        &binding.cassie,
        &session,
        &request,
        &statement,
        b"41\n",
        None,
        binding.owner.as_ref(),
    );
    drop(_controls);
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
