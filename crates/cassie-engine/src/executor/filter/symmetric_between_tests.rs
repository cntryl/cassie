//! Compact trees, durable serialization and query controls for symmetric ranges.
use crate::sql::ast::Expr;

fn nodes(expression: &Expr) -> usize {
    let mut count = 1;
    expression
        .try_visit_children(|child| {
            count += nodes(child);
            Ok::<_, std::convert::Infallible>(())
        })
        .expect("infallible count");
    count
}

#[test]
fn should_keep_nested_symmetric_between_compact_given_repeated_operands_when_parsed() {
    // Arrange
    let mut sql = "TRUE".to_string();
    for _ in 0..16 {
        sql = format!("({sql}) BETWEEN SYMMETRIC FALSE AND TRUE");
    }
    // Act
    let expression = crate::sql::parser::parse_expression(&sql).expect("compact predicate");
    let count = nodes(&expression);
    // Assert
    assert_eq!(count, 49);
}

#[test]
fn should_preserve_legacy_between_serialization_given_asymmetric_input_when_round_tripped() {
    // Arrange
    let old = crate::sql::parser::parse_expression("2 BETWEEN 1 AND 3").expect("ordinary");
    let symmetric =
        crate::sql::parser::parse_expression("2 BETWEEN SYMMETRIC 1 AND 3").expect("symmetric");
    // Act
    let json = serde_json::to_string(&old).expect("serialize");
    let restored: Expr = serde_json::from_str(&json).expect("legacy AST");
    // Assert
    assert!(!json.contains("symmetric"));
    assert!(old.structurally_eq(&restored));
    assert!(!old.structurally_eq(&symmetric));
    assert!(symmetric.structurally_eq(&symmetric.map_children(Clone::clone)));
}

#[test]
fn should_interrupt_nested_constant_ranges_given_deadline_when_comparisons_repeat() {
    // Arrange
    let mut expression = Expr::BoolLiteral(true);
    for _ in 0..28 {
        expression = Expr::Between {
            expr: Box::new(expression),
            low: Box::new(Expr::BoolLiteral(false)),
            high: Box::new(Expr::BoolLiteral(true)),
            negated: false,
            symmetric: true,
        };
    }
    let controls = crate::runtime::QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits {
            query_timeout_ms: 10,
            ..crate::config::CassieRuntimeLimits::default()
        },
        std::time::Instant::now(),
    );
    // Act
    let result = super::constants::constant_scalar_with_parameters(&expression, &[], &controls)
        .map_err(crate::app::CassieError::from);
    // Assert
    assert!(matches!(
        result,
        Err(crate::app::CassieError::DeadlineExceeded)
    ));
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_bound_combined_symmetric_depth_given_cast_chain_when_parsed() {
    // Arrange
    let sql = format!(
        "{} BETWEEN SYMMETRIC FALSE AND TRUE",
        "TRUE".to_owned() + &"::BOOLEAN".repeat(128)
    );
    // Act
    let result = crate::sql::parser::parse_expression(&sql);
    // Assert
    assert_eq!(
        result.expect_err("combined depth must be bounded").kind(),
        crate::sql::parser::SqlErrorKind::ResourceLimit
    );
}

#[test]
fn should_interrupt_runtime_constants_given_session_deadline_when_comparisons_repeat() {
    // Arrange
    let mut expression = Expr::BoolLiteral(true);
    for _ in 0..28 {
        expression = Expr::Between {
            expr: Box::new(expression),
            low: Box::new(Expr::BoolLiteral(false)),
            high: Box::new(Expr::BoolLiteral(true)),
            negated: false,
            symmetric: true,
        };
    }
    let controls = crate::runtime::QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits {
            query_timeout_ms: 10,
            ..crate::config::CassieRuntimeLimits::default()
        },
        std::time::Instant::now(),
    );
    let session = crate::app::CassieSession::new("tester".into(), None)
        .with_statement_text("SELECT nested_ranges", &controls)
        .expect("statement scope");
    let row: Vec<(String, crate::types::Value)> = Vec::new();
    // Act
    let result = super::eval_scalar(
        &row,
        &expression,
        &[],
        None,
        &std::collections::HashMap::new(),
        None,
        Some(&session),
    )
    .map_err(crate::app::CassieError::from);
    // Assert
    assert!(matches!(
        result,
        Err(crate::app::CassieError::DeadlineExceeded)
    ));
    drop(session);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_reject_unselected_symmetric_arrays_given_typed_operands_when_bound() {
    // Arrange
    let statement = crate::sql::parse_statement(
        "SELECT '{2}'::INTEGER[] BETWEEN SYMMETRIC '{3}'::INTEGER[] AND '{1}'::INTEGER[]",
    )
    .expect("array predicate syntax");
    // Act
    let result = crate::sql::bind(statement, &crate::catalog::Catalog::new());
    // Assert
    assert!(matches!(
        result,
        Err(crate::app::CassieError::Unsupported(_))
    ));
}
