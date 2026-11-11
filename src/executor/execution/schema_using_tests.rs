use super::*;
use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits};
use crate::runtime::QueryCancellationHandle;
use std::time::Instant;

fn fixture() -> (CassieSession, Cassie) {
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("using-array-setup", None);
    for sql in [
        "CREATE TABLE char_using(id INT PRIMARY KEY,items CHAR(3)[],amount TEXT)",
        "INSERT INTO char_using VALUES(1,'[-2:-1]={x,NULL}'::CHAR(3)[],'0')",
    ] {
        cassie
            .execute_sql(&session, sql, vec![])
            .expect("small source fixture");
    }
    (session, cassie)
}

fn assert_unmodified(cassie: &Cassie, session: &CassieSession) {
    let result = cassie
        .execute_sql(session, "SELECT amount FROM char_using", vec![])
        .expect("original amount");
    assert_eq!(result.rows(), &[vec![Value::String("0".into())]]);
}

#[test]
fn should_retain_using_output_admission_until_prepared_values_are_released() {
    // Arrange
    let (session, cassie) = fixture();
    let controls =
        QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), Instant::now());
    let context = Context {
        cassie: &cassie,
        session: Some(&session),
        controls: &controls,
        is_column_store: false,
    };
    let expression = crate::sql::parser::parse_expression("octet_length(items[-2])::NUMERIC")
        .expect("using expression");

    // Act
    let prepared = context
        .evaluate("char_using", "amount", &expression)
        .expect("prepared conversion");
    let retained = controls.current_query_memory_bytes();

    // Assert
    assert!(prepared.accounted_bytes() > 0);
    assert!(retained >= prepared.accounted_bytes());
    assert_eq!(
        prepared.as_slice()[0].1["amount"],
        serde_json::Value::String("3".into())
    );
    assert_unmodified(&cassie, &session);
    drop(prepared);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_using_padding_before_any_rewrite_mutation() {
    // Arrange
    let (session, cassie) = fixture();
    let controls = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 64 * 1024,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let context = Context {
        cassie: &cassie,
        session: Some(&session),
        controls: &controls,
        is_column_store: false,
    };
    let expression =
        crate::sql::parser::parse_expression("octet_length((items::CHAR(1000000)[])[-2])::NUMERIC")
            .expect("using expansion");

    // Act
    let result = context.evaluate("char_using", "amount", &expression);

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(CassieError::ResourceLimit(_)))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert_unmodified(&cassie, &session);
}

#[test]
fn should_cancel_using_preparation_before_rewrite_mutation() {
    // Arrange
    let (session, cassie) = fixture();
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let context = Context {
        cassie: &cassie,
        session: Some(&session),
        controls: &controls,
        is_column_store: false,
    };
    let expression = crate::sql::parser::parse_expression("octet_length(items[-2])::NUMERIC")
        .expect("using expression");
    AFTER_PREPARED.with(|slot| *slot.borrow_mut() = Some(cancellation));

    // Act
    let result = context.evaluate("char_using", "amount", &expression);

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(CassieError::QueryCanceled))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert_unmodified(&cassie, &session);
}

#[test]
fn should_keep_prepared_using_owners_until_the_rewrite_boundary() {
    // Arrange
    let (session, cassie) = fixture();
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let context = Context {
        cassie: &cassie,
        session: Some(&session),
        controls: &controls,
        is_column_store: false,
    };
    let expression = crate::sql::parser::parse_expression("octet_length(items[-2])::NUMERIC")
        .expect("using expression");
    let target = DataType::parse_sql("NUMERIC(20,2)").expect("selected target");
    BEFORE_REWRITE.with(|slot| *slot.borrow_mut() = Some(cancellation));
    AT_REWRITE.with(|slot| slot.set((0, 0, 0)));

    // Act
    let result = context.apply("char_using", "amount", &target, &expression);
    let (prepared, writes, live) = AT_REWRITE.with(std::cell::Cell::get);

    // Assert
    assert!(matches!(
        result,
        Err(QueryError::Cassie(CassieError::QueryCanceled))
    ));
    assert!(prepared > 0);
    assert!(writes > 0);
    assert!(live >= prepared + writes);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert_unmodified(&cassie, &session);
}
