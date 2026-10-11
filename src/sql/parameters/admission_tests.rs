use std::time::{Duration, Instant};

use super::*;
use crate::catalog::Catalog;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryCancellationHandle;
use crate::sql::parameters::{
    checked_parse_parameter_types, declared_parameter_type_conflict,
    parameter_type_oids_with_catalog,
};
use crate::sql::{parameter_count, parse_statement};

fn controls(budget: usize) -> QueryExecutionControls {
    QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: budget,
            query_timeout_ms: 0,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    )
}

fn expired_deadline() -> Instant {
    Instant::now()
        .checked_sub(Duration::from_secs(1))
        .expect("past deadline")
}

fn parameter_statement(index: usize) -> ParsedStatement {
    let mut statement = parse_statement("SELECT 1").expect("fixture SQL");
    let QueryStatement::Select(select) = &mut statement.statement else {
        panic!("SELECT fixture");
    };
    select.projection = vec![ast::SelectItem::Expr {
        expr: ast::Expr::Param(index),
        alias: None,
    }];
    statement
}

#[test]
fn should_reject_parameter_extent_overflow_before_slot_allocation() {
    // Arrange
    let statement = parameter_statement(usize::MAX);
    let controls = controls(1_024);
    // Act
    let result = checked_parameter_count(&statement, &controls);
    // Assert
    assert!(matches!(
        result,
        Err(ParameterAdmissionError::InvalidParam(_))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert!(controls.peak_query_memory_bytes() <= 1_024);
}

#[test]
fn should_count_maximum_parameter_slot_without_allocating_slot_storage() {
    // Arrange
    let statement = parameter_statement(MAX_PARAMETER_COUNT - 1);
    let controls = controls(1_024);
    // Act
    let result = checked_parameter_count(&statement, &controls);
    // Assert
    assert_eq!(
        result.expect("logical maximum is valid"),
        MAX_PARAMETER_COUNT
    );
    assert!(controls.peak_query_memory_bytes() > 0);
    assert!(controls.peak_query_memory_bytes() <= 1_024);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_parameter_above_postgresql_logical_bound_without_slot_allocation() {
    // Arrange
    let statement = parameter_statement(MAX_PARAMETER_COUNT);
    let controls = controls(1_024);
    // Act
    let result = checked_parameter_count(&statement, &controls);
    // Assert
    assert!(matches!(
        result,
        Err(ParameterAdmissionError::InvalidParam(_))
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_match_existing_parameter_counts_across_query_contexts() {
    // Arrange
    let queries = [
        "SELECT 1",
        "SELECT $2, $2, $1",
        "EXPLAIN SELECT $3 WHERE $1 LIMIT $4 OFFSET $2",
        "WITH c AS (SELECT $3 AS n) SELECT $1 FROM c WHERE $2",
        "WITH RECURSIVE c(n) AS (SELECT $2 UNION ALL SELECT n + $4 FROM c WHERE n < $3) SELECT $1 FROM c",
        "SELECT d.n FROM (SELECT $2 AS n) AS d JOIN (SELECT $3 AS n) AS e ON d.n = $4 WHERE $1",
        "SELECT $1 UNION ALL SELECT $5",
        "SELECT LAG($2) OVER (PARTITION BY $3 ORDER BY $4) FROM records WHERE $1",
        "SELECT CASE $1 WHEN $2 THEN $3 ELSE $4 END, $5 IN ($6, $7), $8 BETWEEN $9 AND $10",
        "SELECT EXISTS (SELECT $3 WHERE $2), NOT $1, CAST($4 AS INT)",
        "INSERT INTO records (n) VALUES ($1), ($2) ON CONFLICT (n) DO UPDATE SET n = $3 WHERE $4 RETURNING $5",
        "INSERT INTO records (n) SELECT $2 RETURNING $3",
        "UPDATE records SET n = $2 WHERE $1 RETURNING $4",
        "DELETE FROM records WHERE $2 RETURNING $3",
        "CALL sample($2, $4)",
    ];
    let controls = controls(64 * 1_024);
    // Act
    let counts = queries.map(|sql| {
        let statement = parse_statement(sql).expect(sql);
        (
            checked_parameter_count(&statement, &controls).expect(sql),
            parameter_count(&statement),
        )
    });
    // Assert
    for (checked, legacy) in counts {
        assert_eq!(checked, legacy);
    }
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_account_transient_parameter_storage_on_same_controls() {
    // Arrange
    let controls = controls(1_024);
    let statement = parse_statement("SELECT CAST($4 AS INT)").expect("SQL");
    let catalog = Catalog::new();
    // Act
    let admitted = checked_parse_parameter_types(&statement, &[23; 4], &catalog, &controls)
        .expect("admitted metadata");
    let retained = controls.current_query_memory_bytes();
    let peak = controls.peak_query_memory_bytes();
    drop(admitted);
    let released = controls.current_query_memory_bytes();
    let peak_after_drop = controls.peak_query_memory_bytes();
    let reused = controls
        .reserve_query_memory(1_024)
        .expect("same budget reusable");
    // Assert
    assert!(retained > 0);
    assert_eq!(released, 0);
    assert_eq!(peak_after_drop, peak);
    assert!(peak >= retained);
    assert_eq!(controls.current_query_memory_bytes(), 1_024);
    drop(reused);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert!(controls.peak_query_memory_bytes() >= peak);
}

#[test]
fn should_release_transient_guard_when_returning_legacy_oid_vector() {
    // Arrange
    let controls = controls(1_024);
    let statement = parse_statement("SELECT CAST($1 AS INT)").expect("SQL");
    let catalog = Catalog::new();
    let admitted = checked_parse_parameter_types(&statement, &[23], &catalog, &controls)
        .expect("admitted metadata");
    // Act
    let oids = admitted.into_vec().expect("final frontier");
    // Assert
    assert_eq!(oids, [23]);
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert!(controls.peak_query_memory_bytes() > 0);
    // The surviving legacy Vec has no retained prepared-memory lease.
}

#[test]
fn should_prefer_cancelled_entry_over_expired_deadline_without_allocation() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let mut controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    controls.deadline = Some(expired_deadline());
    cancellation.cancel();
    let statement = parameter_statement(0);
    // Act
    let result = checked_parameter_count(&statement, &controls);
    // Assert
    assert!(matches!(result, Err(ParameterAdmissionError::Cancelled)));
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert_eq!(controls.peak_query_memory_bytes(), 0);
}

#[test]
fn should_reject_expired_entry_before_allocating_stack_or_seed() {
    // Arrange
    let mut controls = controls(1_024);
    controls.deadline = Some(expired_deadline());
    let statement = parameter_statement(0);
    // Act
    let result = checked_parse_parameter_types(&statement, &[23], &Catalog::new(), &controls);
    // Assert
    assert!(matches!(result, Err(ParameterAdmissionError::Deadline)));
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert_eq!(controls.peak_query_memory_bytes(), 0);
}

#[test]
fn should_retain_stack_capacity_charge_after_pop_until_drop() {
    // Arrange
    let controls = controls(1_024);
    let expr = ast::Expr::Param(0);
    let mut stack = WorkStack::new(&controls).expect("empty stack");
    // Act
    stack
        .push(Frame::new(Node::Expr(&expr)))
        .expect("stack slot");
    let bytes = controls.current_query_memory_bytes();
    stack.frames.pop().expect("owned frame");
    // Assert
    assert!(bytes > 0);
    assert_eq!(
        bytes,
        stack.frames.capacity() * std::mem::size_of::<Frame<'_>>()
    );
    assert_eq!(controls.current_query_memory_bytes(), bytes);
    drop(stack);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_stack_capacity_after_cancelled_growth() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let expr = ast::Expr::Param(0);
    let mut stack = WorkStack::new(&controls).expect("empty stack");
    stack
        .push(Frame::new(Node::Expr(&expr)))
        .expect("initial slot");
    while stack.frames.len() < stack.frames.capacity() {
        stack
            .push(Frame::new(Node::Expr(&expr)))
            .expect("fill actual capacity");
    }
    cancellation.cancel();
    // Act
    let result = stack.push(Frame::new(Node::Expr(&expr)));
    // Assert
    assert!(matches!(result, Err(ParameterAdmissionError::Cancelled)));
    assert!(controls.current_query_memory_bytes() > 0);
    drop(stack);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_all_transient_reservations_after_stack_or_seed_budget_denial() {
    // Arrange
    let statement = parameter_statement(4_095);
    let no_stack = controls(0);
    let no_seed = controls(1_024);
    // Act
    let stack_result = checked_parameter_count(&statement, &no_stack);
    let seed_result = checked_parse_parameter_types(&statement, &[], &Catalog::new(), &no_seed);
    // Assert
    assert!(matches!(
        stack_result,
        Err(ParameterAdmissionError::Memory(_))
    ));
    assert!(matches!(
        seed_result,
        Err(ParameterAdmissionError::Memory(_))
    ));
    assert_eq!(no_stack.current_query_memory_bytes(), 0);
    assert_eq!(no_stack.peak_query_memory_bytes(), 0);
    assert_eq!(no_seed.current_query_memory_bytes(), 0);
    assert!(no_seed.peak_query_memory_bytes() > 0);
    assert!(no_seed.peak_query_memory_bytes() <= 1_024);
}

#[test]
fn should_preserve_legacy_parameter_metadata_results() {
    // Arrange
    let cases: [(&str, &[i32]); 9] = [
        ("SELECT 1", &[23, 25]),
        ("SELECT CAST($1 AS INT)", &[0]),
        ("SELECT NOT $1", &[705]),
        ("SELECT lower($1)", &[0]),
        ("SELECT COALESCE($1, NULL)", &[705]),
        ("SELECT CASE WHEN $1 THEN $2 ELSE $3 END", &[16, 23, 23]),
        ("SELECT 1 WHERE $1", &[25]),
        ("SELECT NOT $1, $1 + 1", &[705]),
        ("SELECT $2", &[0, 23, 25]),
    ];
    let catalog = Catalog::new();
    let controls = controls(64 * 1_024);
    // Act
    for (sql, supplied) in cases {
        let statement = parse_statement(sql).expect(sql);
        let normalized: Vec<_> = supplied
            .iter()
            .map(|oid| if *oid == 0 { 705 } else { *oid })
            .collect();
        let legacy_conflict = declared_parameter_type_conflict(&statement, &normalized, &catalog);
        let checked = checked_parse_parameter_types(&statement, supplied, &catalog, &controls);
        // Assert
        match legacy_conflict {
            Some(message) => assert!(
                matches!(checked, Err(ParameterAdmissionError::LegacyConflict(actual)) if actual == message)
            ),
            None => assert_eq!(
                checked.expect(sql).into_vec().expect("final check"),
                parameter_type_oids_with_catalog(&statement, &normalized, &catalog)
            ),
        }
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}

#[test]
fn should_check_cancellation_again_before_consuming_admitted_oids() {
    // Arrange
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &CassieRuntimeLimits::default(),
        Instant::now(),
        cancellation.clone(),
    );
    let admitted =
        checked_parse_parameter_types(&parameter_statement(0), &[23], &Catalog::new(), &controls)
            .expect("initial admission");
    cancellation.cancel();
    // Act
    let result = admitted.into_vec();
    // Assert
    assert!(matches!(result, Err(ParameterAdmissionError::Cancelled)));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_check_deadline_again_before_consuming_admitted_oids() {
    // Arrange
    let controls = controls(1_024);
    let mut admitted =
        checked_parse_parameter_types(&parameter_statement(0), &[23], &Catalog::new(), &controls)
            .expect("initial admission");
    admitted.controls.deadline = Some(expired_deadline());
    // Act
    let result = admitted.into_vec();
    // Assert
    assert!(matches!(result, Err(ParameterAdmissionError::Deadline)));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_stack_capacity_after_expired_growth() {
    // Arrange
    let controls = controls(1_024);
    let mut expired = controls.clone();
    expired.deadline = Some(expired_deadline());
    let expr = ast::Expr::Param(0);
    let mut stack = WorkStack::new(&controls).expect("initial controls");
    stack
        .push(Frame::new(Node::Expr(&expr)))
        .expect("initial slot");
    while stack.frames.len() < stack.frames.capacity() {
        stack
            .push(Frame::new(Node::Expr(&expr)))
            .expect("fill actual capacity");
    }
    stack.controls = &expired;
    // Act
    let result = stack.push(Frame::new(Node::Expr(&expr)));
    // Assert
    assert!(matches!(result, Err(ParameterAdmissionError::Deadline)));
    assert!(controls.current_query_memory_bytes() > 0);
    drop(stack);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_release_prior_stack_reservation_when_second_growth_is_denied() {
    // Arrange
    let frame_bytes = size_of::<Frame<'_>>();
    let controls = controls(frame_bytes);
    let statement = parameter_statement(0);
    // Act
    let result = checked_parameter_count(&statement, &controls);
    // Assert
    assert!(matches!(result, Err(ParameterAdmissionError::Memory(_))));
    assert_eq!(controls.peak_query_memory_bytes(), frame_bytes);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_account_full_supplied_normalization_even_without_ast_slots() {
    // Arrange
    let statement = parse_statement("SELECT 1").expect("zero slots");
    let controls = controls(1_024);
    // Act
    let result =
        checked_parse_parameter_types(&statement, &[23; 4_096], &Catalog::new(), &controls);
    // Assert
    assert!(matches!(result, Err(ParameterAdmissionError::Memory(_))));
    assert_eq!(controls.current_query_memory_bytes(), 0);
    assert!(controls.peak_query_memory_bytes() > 0);
    assert!(controls.peak_query_memory_bytes() <= 1_024);
}

#[test]
fn should_release_seed_reservation_when_context_demand_allocation_is_denied() {
    // Arrange
    let controls = controls(512);
    let statement = parameter_statement(127);
    // Act
    let result = checked_parse_parameter_types(&statement, &[], &Catalog::new(), &controls);
    // Assert
    assert!(matches!(result, Err(ParameterAdmissionError::Memory(_))));
    // The walk fits below this peak. The OID seed occupies all 512 bytes,
    // so the following demand allocation fails and must release that seed.
    assert_eq!(controls.peak_query_memory_bytes(), 512);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_bound_parameter_work_stack_by_depth_instead_of_projection_width() {
    // Arrange
    let mut wide = parameter_statement(0);
    let QueryStatement::Select(select) = &mut wide.statement else {
        panic!("SELECT fixture");
    };
    select.projection = vec![select.projection[0].clone(); 4_096];
    let wide_controls = controls(1_024);
    let mut deep = parameter_statement(0);
    let QueryStatement::Select(select) = &mut deep.statement else {
        panic!("SELECT fixture");
    };
    let mut expr = ast::Expr::Param(0);
    for _ in 0..64 {
        expr = ast::Expr::Not {
            expr: Box::new(expr),
        };
    }
    select.projection = vec![ast::SelectItem::Expr { expr, alias: None }];
    let deep_controls = controls(64 * 1_024);

    // Act
    let wide_count = checked_parameter_count(&wide, &wide_controls).expect("wide borrowed walk");
    let deep_count = checked_parameter_count(&deep, &deep_controls).expect("deep borrowed walk");

    // Assert
    assert_eq!(wide_count, 1);
    assert_eq!(deep_count, 1);
    assert!(wide_controls.peak_query_memory_bytes() <= 1_024);
    assert!(deep_controls.peak_query_memory_bytes() > wide_controls.peak_query_memory_bytes());
    assert_eq!(wide_controls.current_query_memory_bytes(), 0);
    assert_eq!(deep_controls.current_query_memory_bytes(), 0);
}
