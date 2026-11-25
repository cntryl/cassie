//! Admission and modifier rejection at the fixed BYTEA parameter boundary.
use crate::catalog::Catalog;
use crate::config::CassieRuntimeLimits;
use crate::runtime::QueryExecutionControls;
use crate::sql::parameters::{checked_parse_parameter_types, ParameterAdmissionError};
use crate::sql::parse_statement;

fn controls() -> QueryExecutionControls {
    QueryExecutionControls::from_limits(&CassieRuntimeLimits::default(), std::time::Instant::now())
}

#[test]
fn should_reject_scalar_modifiers_given_fixed_bytea_calls_when_parse_admits_signatures() {
    // Arrange
    let cases = [
        (
            "SELECT encode('\\x00'::BYTEA,'hex') FILTER (WHERE TRUE)",
            "FILTER specified, but encode is not an aggregate function",
        ),
        (
            "SELECT decode(DISTINCT '00','hex')",
            "DISTINCT specified, but decode is not an aggregate function",
        ),
        (
            "SELECT sha256('\\x00'::BYTEA ORDER BY 1)",
            "ORDER BY specified, but sha256 is not an aggregate function",
        ),
        (
            "SELECT lower(DISTINCT 'A')",
            "DISTINCT specified, but lower is not an aggregate function",
        ),
    ];
    let catalog = Catalog::new();
    let controls = controls();
    // Act
    let results = cases.map(|(sql, message)| {
        (
            checked_parse_parameter_types(&parse_statement(sql).unwrap(), &[], &catalog, &controls),
            message,
        )
    });
    // Assert
    for (result, expected) in results {
        assert!(
            matches!(&result,Err(ParameterAdmissionError::NonAggregateModifier { message,.. }) if message==expected),
            "{result:?}"
        );
        drop(result);
    }
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_own_complete_function_diagnostic_given_wrong_signature_when_parse_returns_an_error() {
    // Arrange
    let statement = parse_statement("SELECT get_byte('\\x00'::BYTEA,1::BIGINT)").unwrap();
    let controls = controls();
    let catalog = Catalog::new();
    // Act
    let result = checked_parse_parameter_types(&statement, &[], &catalog, &controls);
    // Assert
    assert!(
        matches!(&result,Err(ParameterAdmissionError::FunctionDiagnostic { sqlstate:"42883",message,.. }) if message=="function get_byte(bytea, bigint) does not exist"),
        "{result:?}"
    );
    assert!(controls.current_query_memory_bytes() > 0);
    drop(result);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
