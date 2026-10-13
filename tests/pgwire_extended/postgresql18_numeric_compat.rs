//! PostgreSQL 18 NUMERIC OID, typmod and codec contracts tracked by #812.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_preserve_numeric_oid_typmod_text_binary_contract() {
    // Arrange
    let sql = "SELECT CAST($1 AS NUMERIC(5, 2))";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types("numeric_value", sql, &[1700])),
        Step::ThroughReady(vec![
            wire::describe_statement_frame("numeric_value"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame("numeric_text_portal", "numeric_value", &["12.34"]),
            wire::describe_portal_frame("numeric_text_portal"),
            wire::execute_frame("numeric_text_portal"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "numeric_binary_portal",
                "numeric_value",
                &[0],
                &[Some(b"12.34")],
                &[1],
            ),
            wire::describe_portal_frame("numeric_binary_portal"),
            wire::execute_frame("numeric_binary_portal"),
            wire::sync_frame(),
        ]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT CAST('10.00' AS NUMERIC(3, 2))",
        )]),
    ];

    // Act
    let batches = run_script("postgresql18-numeric-codecs", steps);

    // Assert
    assert_eq!(
        batches[0].iter().map(|(tag, _)| *tag).collect::<Vec<_>>(),
        vec![b'1']
    );
    let described = &batches[1];
    let parameter_types = described
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_types.1),
        vec![1700]
    );
    let result_columns = described
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("RowDescription");
    let numeric_column = &wire::parse_row_description(&result_columns.1)[0];
    assert_eq!(numeric_column.type_oid, 1700);
    assert_eq!(numeric_column.type_mod, 327_686);

    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("12.34".to_owned())]]
    );
    let binary_description = batches[3]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("binary RowDescription");
    let binary_column = &wire::parse_row_description(&binary_description.1)[0];
    assert_eq!(binary_column.type_oid, 1700);
    assert_eq!(binary_column.format_code, 1);
    let binary_data = batches[3]
        .iter()
        .find(|(tag, _)| *tag == b'D')
        .expect("binary DataRow");
    let mut expected = 1_i16.to_be_bytes().to_vec();
    expected.extend_from_slice(&12_i32.to_be_bytes());
    expected.extend_from_slice(&2_i16.to_be_bytes());
    expected.extend_from_slice(&0_i16.to_be_bytes());
    expected.extend_from_slice(&0_u16.to_be_bytes());
    expected.extend_from_slice(&2_i16.to_be_bytes());
    expected.extend_from_slice(&12_u16.to_be_bytes());
    expected.extend_from_slice(&3400_u16.to_be_bytes());
    assert_eq!(binary_data.1, expected);

    assert_eq!(wire::error_code(&batches[4]).as_deref(), Some("22003"));
}

#[test]
fn should_describe_bigint_aggregate_results_as_numeric() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE issue_812_aggregate_values (value BIGINT)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "INSERT INTO issue_812_aggregate_values VALUES (1), (2), (3), (4), (5)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT SUM(value), AVG(value) FROM issue_812_aggregate_values",
        )]),
    ];

    // Act
    let batches = run_script("postgresql18-bigint-aggregates", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[2]), None);
    let description = batches[2]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("aggregate RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![1700, 1700]
    );
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![
            Some("15".to_owned()),
            Some("3.0000000000000000".to_owned())
        ]]
    );
}
