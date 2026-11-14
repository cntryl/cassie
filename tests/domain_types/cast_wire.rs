//! Casts to a domain over real pgwire: result typing, enforcement and
//! resolution of operators, functions and aggregates through the base type.
//!
//! The in-process fixture does not build a `RowDescription`, so these run a
//! server and read frames.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

fn setup() -> Vec<(&'static str, Vec<cassie::types::Value>)> {
    [
        "CREATE DOMAIN posint AS int CHECK (VALUE > 0)",
        "CREATE DOMAIN label AS text",
        "CREATE DOMAIN flag AS boolean",
        "CREATE DOMAIN doc AS jsonb",
        "CREATE DOMAIN ratio AS float8",
        "CREATE DOMAIN intlist AS int[]",
        "CREATE TABLE items (id int, qty int)",
        "INSERT INTO items VALUES (1, 4)",
        "CREATE VIEW fixed AS SELECT 5::posint AS x",
    ]
    .into_iter()
    .map(|sql| (sql, Vec::new()))
    .collect()
}

/// Runs each statement as one simple-protocol query cycle.
fn simple_queries(label: &str, statements: &[&str]) -> Vec<fixture::Frames> {
    let cycles = statements
        .iter()
        .map(|sql| vec![wire::simple_query_frame(sql)])
        .collect();
    fixture::run_cycles(label, &setup(), cycles)
}

/// The single text value a successful one-row, one-column query returns.
fn single_value(label: &str, sql: &str) -> Option<String> {
    let batches = simple_queries(label, &[sql]);
    fixture::assert_success(&batches[0]);
    let mut rows = wire::data_rows(&batches[0]);
    assert_eq!(rows.len(), 1, "one row for {sql}");
    let mut row = rows.remove(0);
    assert_eq!(row.len(), 1, "one column for {sql}");
    row.remove(0)
}

#[test]
fn should_return_a_domain_cast_over_the_simple_protocol() {
    // Arrange
    let sql = "SELECT 5::posint";

    // Act
    let value = single_value("domain-cast-simple", sql);

    // Assert
    assert_eq!(value, Some("5".to_owned()));
}

#[test]
fn should_return_a_cast_function_to_a_domain() {
    // Arrange
    let sql = "SELECT CAST(5 AS posint)";

    // Act
    let value = single_value("domain-cast-function", sql);

    // Assert
    assert_eq!(value, Some("5".to_owned()));
}

#[test]
fn should_return_null_for_a_null_cast_to_a_nullable_domain() {
    // Arrange
    let sql = "SELECT NULL::posint";

    // Act
    let value = single_value("domain-cast-null", sql);

    // Assert
    assert_eq!(value, None);
}

#[test]
fn should_return_a_column_cast_to_a_domain() {
    // Arrange
    let sql = "SELECT qty::posint FROM items";

    // Act
    let value = single_value("domain-cast-column", sql);

    // Assert
    assert_eq!(value, Some("4".to_owned()));
}

#[test]
fn should_return_a_domain_cast_from_returning() {
    // Arrange
    let sql = "INSERT INTO items VALUES (2, 9) RETURNING qty::posint";

    // Act
    let value = single_value("domain-cast-returning", sql);

    // Assert
    assert_eq!(value, Some("9".to_owned()));
}

#[test]
fn should_return_a_view_column_defined_by_a_domain_cast() {
    // Arrange
    let sql = "SELECT x FROM fixed";

    // Act
    let value = single_value("domain-cast-view", sql);

    // Assert
    assert_eq!(value, Some("5".to_owned()));
}

#[test]
fn should_subscript_a_cast_to_an_array_domain() {
    // Arrange
    let sql = "SELECT (ARRAY[1,2]::intlist)[1]";

    // Act
    let value = single_value("domain-cast-array-element", sql);

    // Assert
    assert_eq!(value, Some("1".to_owned()));
}

#[test]
fn should_resolve_max_over_a_domain_cast() {
    // Arrange
    let sql = "SELECT max(qty::posint) FROM items";

    // Act
    let value = single_value("domain-cast-max", sql);

    // Assert
    assert_eq!(value, Some("4".to_owned()));
}

#[test]
fn should_resolve_sum_over_a_domain_cast() {
    // Arrange
    let sql = "SELECT sum(qty::posint) FROM items";

    // Act
    let value = single_value("domain-cast-sum", sql);

    // Assert
    assert_eq!(value, Some("4".to_owned()));
}

#[test]
fn should_resolve_string_agg_over_a_domain_cast() {
    // Arrange
    let sql = "SELECT string_agg('a'::label, ',') FROM items";

    // Act
    let value = single_value("domain-cast-string-agg", sql);

    // Assert
    assert_eq!(value, Some("a".to_owned()));
}

#[test]
fn should_resolve_coalesce_over_a_domain_cast() {
    // Arrange
    let sql = "SELECT coalesce(5::posint, 1)";

    // Act
    let value = single_value("domain-cast-coalesce", sql);

    // Assert
    assert_eq!(value, Some("5".to_owned()));
}

#[test]
fn should_resolve_greatest_over_a_domain_cast() {
    // Arrange
    let sql = "SELECT greatest(5::posint, 7)";

    // Act
    let value = single_value("domain-cast-greatest", sql);

    // Assert
    assert_eq!(value, Some("7".to_owned()));
}

#[test]
fn should_concatenate_a_text_domain_cast() {
    // Arrange
    let sql = "SELECT 'a'::label || 'b'";

    // Act
    let value = single_value("domain-cast-concat", sql);

    // Assert
    assert_eq!(value, Some("ab".to_owned()));
}

#[test]
fn should_combine_a_boolean_domain_cast_with_and() {
    // Arrange
    let sql = "SELECT true::flag AND true";

    // Act
    let value = single_value("domain-cast-and", sql);

    // Assert
    assert_eq!(value, Some("t".to_owned()));
}

#[test]
fn should_extract_a_key_from_a_jsonb_domain_cast() {
    // Arrange
    let sql = r#"SELECT '{"a":1}'::doc -> 'a' "#;

    // Act
    let value = single_value("domain-cast-json-extract", sql);

    // Assert
    assert_eq!(value, Some("1".to_owned()));
}

#[test]
fn should_add_to_a_decimal_literal_cast_to_a_float_domain() {
    // Arrange
    let sql = "SELECT 1.5::ratio + 0";

    // Act
    let value = single_value("domain-cast-decimal", sql);

    // Assert
    assert_eq!(value, Some("1.5".to_owned()));
}

#[test]
fn should_unify_a_domain_cast_in_case_branches() {
    // Arrange
    let sql = "SELECT CASE WHEN true THEN 5::posint ELSE 2 END";

    // Act
    let value = single_value("domain-cast-case", sql);

    // Assert
    assert_eq!(value, Some("5".to_owned()));
}

#[test]
fn should_describe_a_domain_cast_with_the_base_oid() {
    // Arrange
    let cycles = vec![fixture::execute_cycle("SELECT 5::posint", None, 0, true)];

    // Act
    let batches = fixture::run_cycles("domain-cast-oid", &setup(), cycles);

    // Assert
    fixture::assert_success(&batches[0]);
    fixture::assert_single_column_descriptors(&batches[0], (23, 4, -1), 0, true);
}

#[test]
fn should_return_a_parameter_cast_to_a_domain_over_the_extended_protocol() {
    // Arrange
    let cycles = vec![fixture::execute_cycle(
        "SELECT $1::posint",
        Some((23, 0, Some(b"7".as_slice()))),
        0,
        false,
    )];

    // Act
    let batches = fixture::run_cycles("domain-cast-extended", &setup(), cycles);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("7".to_owned())]]
    );
}

#[test]
fn should_reject_a_violating_parameter_cast_with_sqlstate_23514() {
    // Arrange
    let cycles = vec![fixture::execute_cycle(
        "SELECT $1::posint",
        Some((23, 0, Some(b"-7".as_slice()))),
        0,
        false,
    )];

    // Act
    let batches = fixture::run_cycles("domain-cast-extended-violation", &setup(), cycles);

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("23514"));
}

#[test]
fn should_reject_a_violating_literal_cast_with_sqlstate_23514() {
    // Arrange
    let statements = ["SELECT (-5)::posint"];

    // Act
    let batches = simple_queries("domain-cast-violation", &statements);

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("23514"));
}

#[test]
fn should_execute_a_prepared_statement_that_casts_to_a_domain() {
    // Arrange
    let statements = [
        "PREPARE checked(int) AS SELECT $1::posint",
        "EXECUTE checked(4)",
    ];

    // Act
    let batches = simple_queries("domain-cast-prepare", &statements);

    // Assert
    fixture::assert_success(&batches[1]);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("4".to_owned())]]
    );
}

#[test]
fn should_reject_a_violating_prepared_domain_cast_with_sqlstate_23514() {
    // Arrange
    let statements = [
        "PREPARE checked(int) AS SELECT $1::posint",
        "EXECUTE checked(-4)",
    ];

    // Act
    let batches = simple_queries("domain-cast-prepare-violation", &statements);

    // Assert
    assert_eq!(wire::error_code(&batches[1]).as_deref(), Some("23514"));
}
