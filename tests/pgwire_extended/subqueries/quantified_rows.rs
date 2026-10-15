//! Row-valued ANY/ALL three-valued logic and empty-set identities (#762/#800).

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

fn evaluate_row_quantifier(sql: &str, label: &str) -> Vec<Vec<(u8, Vec<u8>)>> {
    run_script(
        label,
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE quantified_row_candidates (id INT4 PRIMARY KEY, first_value INT4, second_value INT4)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO quantified_row_candidates VALUES (1, 1, 2), (2, 2, 1)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT first_value, second_value FROM quantified_row_candidates ORDER BY id",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(sql)]),
        ],
    )
}

fn assert_table_control(frames: &[(u8, Vec<u8>)]) {
    assert_eq!(
        wire::error_code(frames),
        None,
        "quantified-row table control ErrorResponse: {:?}",
        frames
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![Some("1".to_owned()), Some("2".to_owned())],
            vec![Some("2".to_owned()), Some("1".to_owned())],
        ]
    );
}

fn assert_quantifier_result(frames: &[(u8, Vec<u8>)], expected: Option<&str>, context: &str) {
    assert_eq!(
        wire::error_code(frames),
        None,
        "{context} ErrorResponse: {:?}",
        frames
            .iter()
            .filter(|frame| frame.0 == b'E')
            .map(|frame| wire::parse_error_fields(&frame.1))
            .collect::<Vec<_>>()
    );
    let description = frames
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("row-valued quantifier Boolean RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 16);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![expected.map(str::to_owned)]],
        "PostgreSQL 18.6 result for {context}"
    );
}

#[test]
fn should_return_unknown_given_null_row_candidates_when_any_has_no_match() {
    // Arrange
    let batches = evaluate_row_quantifier(
        "SELECT ROW(1, NULL::INT4) = ANY (SELECT first_value, second_value FROM quantified_row_candidates)",
        "row-any-null-candidate",
    );

    // Act
    let control = &batches[2];
    let quantified = &batches[3];

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_table_control(control);
    assert_quantifier_result(quantified, None, "row ANY with an UNKNOWN candidate");
}

#[test]
fn should_return_false_given_an_empty_row_subquery_when_any_is_evaluated() {
    // Arrange
    let batches = evaluate_row_quantifier(
        "SELECT ROW(1, NULL::INT4) = ANY (SELECT first_value, second_value FROM quantified_row_candidates WHERE false)",
        "row-any-empty-candidate",
    );

    // Act
    let control = &batches[2];
    let quantified = &batches[3];

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_table_control(control);
    assert_quantifier_result(quantified, Some("f"), "row ANY over the empty set");
}

#[test]
fn should_return_unknown_given_null_row_candidates_when_all_has_no_false_result() {
    // Arrange
    let batches = evaluate_row_quantifier(
        "SELECT ROW(1, NULL::INT4) = ALL (SELECT first_value, second_value FROM quantified_row_candidates WHERE id = 1)",
        "row-all-null-candidate",
    );

    // Act
    let control = &batches[2];
    let quantified = &batches[3];

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_table_control(control);
    assert_quantifier_result(quantified, None, "row ALL with an UNKNOWN candidate");
}

#[test]
fn should_return_true_given_an_empty_row_subquery_when_all_is_evaluated() {
    // Arrange
    let batches = evaluate_row_quantifier(
        "SELECT ROW(1, NULL::INT4) = ALL (SELECT first_value, second_value FROM quantified_row_candidates WHERE false)",
        "row-all-empty-candidate",
    );

    // Act
    let control = &batches[2];
    let quantified = &batches[3];

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_table_control(control);
    assert_quantifier_result(quantified, Some("t"), "row ALL over the empty set");
}
