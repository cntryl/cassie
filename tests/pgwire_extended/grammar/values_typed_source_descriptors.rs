//! Derived VALUES descriptors with inferred mixed NULL column types.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_describe_derived_values_default_columns_with_mixed_nulls() {
    // Arrange
    let sql = "SELECT * FROM (VALUES (NULL, 'alpha'), (2, NULL)) AS v ORDER BY column1 NULLS FIRST";
    let batches = run_script(
        "issue-800-values-default-column-descriptor",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("derived VALUES RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["column1", "column2"]
    );
    assert_eq!(
        columns
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        [(23, 4, -1), (25, -1, -1)]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![
            vec![None, Some("alpha".to_owned())],
            vec![Some("2".to_owned()), None],
        ]
    );
}

#[test]
fn should_preserve_exact_bigint_values_given_unknown_values_parameter_oids_when_binary_portals_execute(
) {
    // Arrange
    let input = 7_i64.to_be_bytes();
    let batches = run_script(
        "values-binary-bigint",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "values",
                "VALUES ($1), (9007199254740993::BIGINT)",
                &[0],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("values"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats("out", "values", &[1], &[Some(&input)], &[1]),
                wire::describe_portal_frame("out"),
                wire::execute_frame("out"),
                wire::sync_frame(),
            ]),
        ],
    );
    // Act
    let parameters = batches[1].iter().find(|frame| frame.0 == b't');
    let rows = batches[2]
        .iter()
        .filter(|frame| frame.0 == b'D')
        .collect::<Vec<_>>();
    // Assert
    for batch in &batches {
        assert_eq!(wire::error_code(batch), None);
    }
    assert_eq!(
        wire::parse_parameter_description(&parameters.expect("parameter description").1),
        vec![20]
    );
    for batch in &batches[1..] {
        let description = batch
            .iter()
            .find(|frame| frame.0 == b'T')
            .expect("VALUES description");
        assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 20);
    }
    assert_eq!(rows.len(), 2);
    for (row, expected) in rows.iter().zip([7_i64, 9_007_199_254_740_993]) {
        assert_eq!(&row.1[2..6], &8_i32.to_be_bytes());
        assert_eq!(&row.1[6..], &expected.to_be_bytes());
    }
}

#[test]
fn should_preserve_unconstrained_character_parameters_given_a_values_type_context_when_parse_and_bind_execute(
) {
    // Arrange
    let text = "long🙂 ";
    let batches = run_script(
        "values-character-parameters",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "values",
                "VALUES ($1), ('a'::CHAR(3))",
                &[0],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("values"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats("out", "values", &[], &[Some(text.as_bytes())], &[]),
                wire::execute_frame("out"),
                wire::sync_frame(),
            ]),
        ],
    );
    // Act
    let parameters = batches[1].iter().find(|frame| frame.0 == b't');
    let rows = wire::data_rows(&batches[2]);
    // Assert
    for batch in &batches {
        assert_eq!(wire::error_code(batch), None);
    }
    assert_eq!(
        wire::parse_parameter_description(&parameters.expect("CHAR ParameterDescription").1),
        vec![1042]
    );
    let description = batches[1]
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("CHAR RowDescription");
    let column = &wire::parse_row_description(&description.1)[0];
    assert_eq!((column.type_oid, column.type_mod), (1042, -1));
    assert_eq!(
        rows,
        vec![vec![Some(text.into())], vec![Some("a  ".into())]]
    );
}
