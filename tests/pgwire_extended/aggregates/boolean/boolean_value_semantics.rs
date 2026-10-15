//! PostgreSQL 18 Boolean aggregate values and result metadata for #805.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_return_boolean_aggregate_truth_table() {
    // Arrange
    let mut steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TABLE issue_805_boolean_truth (bucket TEXT, flag BOOLEAN)",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "INSERT INTO issue_805_boolean_truth VALUES \
             ('all-null', NULL), ('all-null', NULL), \
             ('all-true', TRUE), ('all-true', TRUE), \
             ('false', FALSE), ('false', FALSE), \
             ('mixed', TRUE), ('mixed', FALSE), ('mixed', NULL)",
        )]),
    ];
    steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT bucket, BOOL_AND(flag), BOOL_OR(flag), EVERY(flag) \
         FROM issue_805_boolean_truth GROUP BY bucket ORDER BY bucket",
    )]));

    // Act
    let batches = run_script("issue-805-boolean-aggregate-values", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(&batches[1]), None);
    let result = &batches[2];
    let error = result
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .map(|(_, payload)| wire::parse_error_fields(payload));
    assert_eq!(
        wire::error_code(result),
        None,
        "ErrorResponse fields: {error:?}"
    );
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("Boolean aggregate RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 16, 16, 16]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![
            vec![Some("all-null".to_owned()), None, None, None],
            vec![
                Some("all-true".to_owned()),
                Some("t".to_owned()),
                Some("t".to_owned()),
                Some("t".to_owned()),
            ],
            vec![
                Some("false".to_owned()),
                Some("f".to_owned()),
                Some("f".to_owned()),
                Some("f".to_owned()),
            ],
            vec![
                Some("mixed".to_owned()),
                Some("f".to_owned()),
                Some("t".to_owned()),
                Some("f".to_owned()),
            ],
        ]
    );
}

#[test]
fn should_preserve_boolean_parameter_and_result_identity_given_oid_zero_and_binary_bind_when_aggregates_execute(
) {
    // Arrange
    let mut steps = Vec::new();
    for name in ["bool_and", "bool_or", "every"] {
        for value in [Some(0_u8), Some(1_u8), None] {
            let payload = value.map(|value| [value]);
            steps.push(Step::ThroughReady(vec![
                wire::parse_frame_with_types("", &format!("SELECT {name}($1)"), &[0]),
                wire::describe_statement_frame(""),
                wire::bind_frame_with_formats(
                    "",
                    "",
                    &[1],
                    &[payload.as_ref().map(|value| &value[..])],
                    &[1],
                ),
                wire::describe_portal_frame(""),
                wire::execute_frame(""),
                wire::sync_frame(),
            ]));
        }
    }

    // Act
    let batches = run_script("issue-805-boolean-binary-parameters", steps);

    // Assert
    for (index, frames) in batches.iter().enumerate() {
        assert_eq!(wire::error_code(frames), None, "cycle {index}");
        let parameter = frames
            .iter()
            .find(|(tag, _)| *tag == b't')
            .expect("ParameterDescription");
        assert_eq!(wire::parse_parameter_description(&parameter.1), [16]);
        let descriptions = frames
            .iter()
            .filter(|(tag, _)| *tag == b'T')
            .map(|(_, payload)| wire::parse_row_description(payload))
            .collect::<Vec<_>>();
        assert_eq!(descriptions.len(), 2);
        for (description, format) in descriptions.iter().zip([0, 1]) {
            assert_eq!(description[0].type_oid, 16);
            assert_eq!(description[0].type_size, 1);
            assert_eq!(description[0].format_code, format);
        }
        let expected = match index % 3 {
            0 => vec![0, 1, 0, 0, 0, 1, 0],
            1 => vec![0, 1, 0, 0, 0, 1, 1],
            _ => vec![0, 1, 255, 255, 255, 255],
        };
        let rows = frames
            .iter()
            .filter(|(tag, _)| *tag == b'D')
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1, expected);
    }
}

#[test]
fn should_preserve_declared_scalar_calls_given_boolean_aggregate_names_when_scoped_routines_execute(
) {
    // Arrange
    let mut steps = ["bool_and", "bool_or", "every"]
        .into_iter()
        .map(|name| {
            Step::ThroughReady(vec![wire::simple_query_frame(&format!(
                "CREATE FUNCTION {name}(value INT) RETURNS INT AS 'value + 1'"
            ))])
        })
        .collect::<Vec<_>>();
    steps.push(Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT bool_and(7), public.bool_and(8), bool_or(9), \
                public.bool_or(10), every(11), public.every(12)",
    )]));
    let parameter = 20_i32.to_be_bytes();
    steps.push(Step::ThroughReady(vec![
        wire::parse_frame_with_types("", "SELECT bool_and($1)", &[0]),
        wire::describe_statement_frame(""),
        wire::bind_frame_with_formats("", "", &[1], &[Some(&parameter)], &[1]),
        wire::describe_portal_frame(""),
        wire::execute_frame(""),
        wire::sync_frame(),
    ]));

    // Act
    let batches = run_script("issue-805-existing-scalar-precedence", steps);

    // Assert
    for frames in &batches {
        assert_eq!(wire::error_code(frames), None);
    }
    let description = batches[3]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("scalar descriptors");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.name.as_str(), column.type_oid))
            .collect::<Vec<_>>(),
        [
            ("bool_and", 23),
            ("public.bool_and", 23),
            ("bool_or", 23),
            ("public.bool_or", 23),
            ("every", 23),
            ("public.every", 23),
        ],
    );
    assert_eq!(
        wire::data_rows(&batches[3]),
        vec![vec![
            Some("8".to_owned()),
            Some("9".to_owned()),
            Some("10".to_owned()),
            Some("11".to_owned()),
            Some("12".to_owned()),
            Some("13".to_owned())
        ]],
    );
    let prepared = &batches[4];
    let parameter = prepared
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("declared scalar parameter");
    assert_eq!(wire::parse_parameter_description(&parameter.1), [23]);
    for (_, payload) in prepared.iter().filter(|(tag, _)| *tag == b'T') {
        let columns = wire::parse_row_description(payload);
        assert_eq!(columns[0].name, "bool_and");
        assert_eq!(columns[0].type_oid, 23);
    }
    let row = prepared
        .iter()
        .find(|(tag, _)| *tag == b'D')
        .expect("prepared scalar row");
    assert_eq!(row.1, [0, 1, 0, 0, 0, 4, 0, 0, 0, 21]);
}
