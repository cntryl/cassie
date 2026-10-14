//! PostgreSQL Parse, Describe, Bind and Execute behavior for #762 subqueries.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

fn setup() -> Step {
    Step::ThroughReady(vec![wire::simple_query_frame(
        "CREATE TABLE scalar_wire_values (id INT, value INT); \
         INSERT INTO scalar_wire_values (id, value) VALUES (1, 100), (2, 200)",
    )])
}

fn predicate_setup() -> Step {
    Step::ThroughReady(vec![wire::simple_query_frame(
        "CREATE TABLE subquery_predicate_values (id INT, n INT); \
         INSERT INTO subquery_predicate_values (id, n) \
         VALUES (1, 100), (2, 200), (3, NULL)",
    )])
}

fn error_summary(batch: &[(u8, Vec<u8>)]) -> Option<String> {
    let frame = batch.iter().find(|frame| frame.0 == b'E')?;
    let fields = wire::parse_error_fields(&frame.1);
    Some(
        fields
            .iter()
            .map(|(kind, value)| format!("{kind}={value}"))
            .collect::<Vec<_>>()
            .join(", "),
    )
}

#[test]
fn should_execute_scalar_subqueries_in_simple_query_protocol() {
    // Arrange
    let batches = run_script(
        "issue-762-scalar-simple-query",
        vec![
            setup(),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT (SELECT 17 AS scalar_value) AS scalar_value",
            )]),
        ],
    );

    // Act
    let result = &batches[1];

    // Assert
    assert_eq!(wire::error_code(result), None);
    assert_eq!(wire::data_rows(result), vec![vec![Some("17".into())]]);
}

#[test]
fn should_filter_rows_with_a_scalar_subquery_predicate() {
    // Arrange
    let batches = run_script(
        "issue-762-scalar-predicate-wire",
        vec![
            setup(),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT id FROM scalar_wire_values WHERE value = (SELECT 100) ORDER BY id",
            )]),
        ],
    );

    // Act
    let result = &batches[1];

    // Assert
    assert_eq!(wire::error_code(result), None);
    assert_eq!(wire::data_rows(result), vec![vec![Some("1".into())]]);
}

#[test]
fn should_return_a_null_value_from_a_one_row_null_scalar_subquery() {
    // Arrange
    let batches = run_script(
        "issue-762-scalar-null-value-wire",
        vec![
            setup(),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT (SELECT NULL::INT4) AS scalar_value",
            )]),
        ],
    );

    // Act
    let result = &batches[1];

    // Assert
    assert_eq!(wire::error_code(result), None);
    assert_eq!(wire::data_rows(result), vec![vec![None]]);
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("NULL scalar RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 23);
}

#[test]
fn should_raise_cardinality_violation_in_each_scalar_expression_position() {
    // Arrange
    let cases = [
        (
            "SELECT id FROM scalar_wire_values WHERE id = (SELECT id FROM scalar_wire_values)",
            "21000",
        ),
        (
            "SELECT id FROM scalar_wire_values ORDER BY (SELECT id FROM scalar_wire_values)",
            "21000",
        ),
        (
            "SELECT id FROM scalar_wire_values GROUP BY id HAVING (SELECT id FROM scalar_wire_values) > 0",
            "21000",
        ),
        ("SELECT abs((SELECT id FROM scalar_wire_values))", "21000"),
        (
            "SELECT CASE WHEN TRUE THEN (SELECT id FROM scalar_wire_values) ELSE 0 END",
            "21000",
        ),
        (
            "SELECT left_value.id FROM scalar_wire_values AS left_value JOIN scalar_wire_values AS right_value ON right_value.id = (SELECT id FROM scalar_wire_values)",
            "21000",
        ),
    ];
    let mut steps = vec![setup()];
    steps.extend(
        cases
            .iter()
            .map(|(sql, _)| Step::ThroughReady(vec![wire::simple_query_frame(sql)])),
    );

    // Act
    let batches = run_script("issue-762-scalar-position-cardinality-wire", steps);
    let failures = cases
        .iter()
        .enumerate()
        .filter_map(|(index, (sql, expected))| {
            let actual = wire::error_code(&batches[index + 1]);
            (actual.as_deref() != Some(*expected))
                .then(|| format!("{sql}: expected SQLSTATE {expected}, got {actual:?}"))
        })
        .collect::<Vec<_>>();

    // Assert
    assert!(
        failures.is_empty(),
        "scalar cardinality state differs by expression position:\n{}",
        failures.join("\n")
    );
}

#[test]
fn should_reject_a_multi_column_scalar_subquery_with_syntax_sqlstate() {
    // Arrange
    let batches = run_script(
        "issue-762-scalar-width-wire",
        vec![
            setup(),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT (SELECT id, value FROM scalar_wire_values WHERE id = 1)",
            )]),
        ],
    );

    // Act
    let result = &batches[1];

    // Assert
    assert_eq!(wire::error_code(result).as_deref(), Some("42601"));
}

#[test]
fn should_report_row_subquery_arity_errors_with_postgres_syntax_sqlstate() {
    // Arrange
    let batches = run_script(
        "issue-762-row-subquery-arity-wire",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT ROW(1::INT4, 2::INT4) = (SELECT 1::INT4)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT ROW(1::INT4, 2::INT4) IN (SELECT 1::INT4)",
            )]),
        ],
    );

    // Act
    let errors = batches
        .iter()
        .map(|batch| {
            let frame = batch
                .iter()
                .find(|frame| frame.0 == b'E')
                .expect("row-subquery arity ErrorResponse");
            wire::parse_error_fields(&frame.1)
        })
        .collect::<Vec<_>>();

    // Assert
    for fields in errors {
        assert_eq!(
            fields
                .iter()
                .find(|(kind, _)| *kind == 'C')
                .map(|(_, value)| value.as_str()),
            Some("42601")
        );
        assert_eq!(
            fields
                .iter()
                .find(|(kind, _)| *kind == 'M')
                .map(|(_, value)| value.as_str()),
            Some("subquery has too few columns")
        );
    }
}

#[test]
fn should_describe_scalar_subquery_parameter_metadata() {
    // Arrange
    let batches = run_script(
        "issue-762-scalar-describe",
        vec![
            setup(),
            Step::ParseFlush(wire::parse_frame_with_types(
                "scalar_subquery",
                "SELECT (SELECT value FROM scalar_wire_values WHERE id = $1) AS scalar_value",
                &[0],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("scalar_subquery"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let parsed = &batches[1];
    let described = &batches[2];

    // Assert
    assert_eq!(
        parsed.iter().map(|frame| frame.0).collect::<Vec<_>>(),
        vec![b'1']
    );
    assert_eq!(wire::error_code(described), None);
    let parameters = described
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("ParameterDescription");
    assert_eq!(wire::parse_parameter_description(&parameters.1), vec![23]);
    let description = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(columns.len(), 1);
    assert_eq!(columns[0].name, "scalar_value");
    assert_eq!(columns[0].type_oid, 23);
}

#[test]
fn should_bind_text_parameters_to_scalar_subqueries() {
    // Arrange
    let batches = run_script(
        "issue-762-scalar-text-bind",
        vec![
            setup(),
            Step::ParseFlush(wire::parse_frame_with_types(
                "scalar_subquery",
                "SELECT (SELECT value FROM scalar_wire_values WHERE id = $1) AS scalar_value",
                &[0],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("scalar_subquery"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame("scalar_text", "scalar_subquery", &["1"]),
                wire::execute_frame("scalar_text"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let parsed = &batches[1];
    let result = &batches[3];

    // Assert
    assert_eq!(
        parsed.iter().map(|frame| frame.0).collect::<Vec<_>>(),
        vec![b'1']
    );
    assert_eq!(wire::error_code(result), None);
    assert!(
        !result.iter().any(|frame| frame.0 == b'T'),
        "Execute without portal Describe must not send RowDescription"
    );
    assert_eq!(wire::data_rows(result), vec![vec![Some("100".into())]]);
}

#[test]
fn should_return_cardinality_violation_for_multiple_scalar_rows_on_the_wire() {
    // Arrange
    let batches = run_script(
        "issue-762-scalar-cardinality-sqlstate",
        vec![
            setup(),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT (SELECT value FROM scalar_wire_values) AS scalar_value",
            )]),
        ],
    );

    // Act
    let result = &batches[1];

    // Assert
    assert_eq!(wire::error_code(result).as_deref(), Some("21000"));
    assert_eq!(wire::data_rows(result), Vec::<Vec<Option<String>>>::new());
}

#[test]
fn should_preserve_column_metadata_for_empty_scalar_subquery_results() {
    // Arrange
    let batches = run_script(
        "issue-762-scalar-empty-result-metadata",
        vec![
            setup(),
            Step::ParseFlush(wire::parse_frame_with_types(
                "scalar_subquery",
                "SELECT (SELECT value FROM scalar_wire_values WHERE id = $1) AS scalar_value",
                &[0],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("scalar_subquery"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame("scalar_empty", "scalar_subquery", &["99"]),
                wire::describe_portal_frame("scalar_empty"),
                wire::execute_frame("scalar_empty"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let parsed = &batches[1];
    let result = &batches[3];

    // Assert
    assert_eq!(
        parsed.iter().map(|frame| frame.0).collect::<Vec<_>>(),
        vec![b'1']
    );
    assert_eq!(wire::error_code(result), None);
    assert_eq!(wire::data_rows(result), vec![vec![None]]);
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("empty scalar RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 23);
}

#[test]
fn should_bind_binary_parameters_to_scalar_subqueries() {
    // Arrange
    let batches = run_script(
        "issue-762-scalar-binary-bind",
        vec![
            setup(),
            Step::ParseFlush(wire::parse_frame_with_types(
                "scalar_subquery",
                "SELECT (SELECT value FROM scalar_wire_values WHERE id = $1) AS scalar_value",
                &[0],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("scalar_subquery"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    "scalar_binary",
                    "scalar_subquery",
                    &[1],
                    &[Some(&1_i32.to_be_bytes())],
                    &[1],
                ),
                wire::describe_portal_frame("scalar_binary"),
                wire::execute_frame("scalar_binary"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let parsed = &batches[1];
    let result = &batches[3];

    // Assert
    assert_eq!(
        parsed.iter().map(|frame| frame.0).collect::<Vec<_>>(),
        vec![b'1']
    );
    assert_eq!(wire::error_code(result), None);
    let binary_description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("binary RowDescription");
    let column = &wire::parse_row_description(&binary_description.1)[0];
    assert_eq!(column.type_oid, 23);
    assert_eq!(column.format_code, 1);
    let data_row = result
        .iter()
        .find(|frame| frame.0 == b'D')
        .expect("binary DataRow");
    assert_eq!(
        i16::from_be_bytes(data_row.1[0..2].try_into().expect("field count")),
        1
    );
    assert_eq!(
        i32::from_be_bytes(data_row.1[2..6].try_into().expect("field length")),
        4
    );
    assert_eq!(
        i32::from_be_bytes(data_row.1[6..10].try_into().expect("binary INT4")),
        100
    );
}

#[test]
fn should_match_subquery_predicate_truth_table() {
    // Arrange
    let cases = [
        (
            "SELECT 100 IN (SELECT n FROM subquery_predicate_values)",
            vec![vec![Some("t")]],
            16,
        ),
        (
            "SELECT 150 IN (SELECT n FROM subquery_predicate_values)",
            vec![vec![None]],
            16,
        ),
        (
            "SELECT 150 NOT IN (SELECT n FROM subquery_predicate_values)",
            vec![vec![None]],
            16,
        ),
        (
            "SELECT 100 NOT IN (SELECT n FROM subquery_predicate_values)",
            vec![vec![Some("f")]],
            16,
        ),
        (
            "SELECT 100 = ANY (SELECT n FROM subquery_predicate_values)",
            vec![vec![Some("t")]],
            16,
        ),
        (
            "SELECT 150 = ANY (SELECT n FROM subquery_predicate_values)",
            vec![vec![None]],
            16,
        ),
        (
            "SELECT 150 = ALL (SELECT n FROM subquery_predicate_values)",
            vec![vec![Some("f")]],
            16,
        ),
        (
            "SELECT 100 = ALL (SELECT n FROM subquery_predicate_values \
             WHERE n IS NULL OR n = 100)",
            vec![vec![None]],
            16,
        ),
        (
            "SELECT 150 IN (SELECT n FROM subquery_predicate_values WHERE false)",
            vec![vec![Some("f")]],
            16,
        ),
        (
            "SELECT 150 NOT IN (SELECT n FROM subquery_predicate_values WHERE false)",
            vec![vec![Some("t")]],
            16,
        ),
        (
            "SELECT 150 = ANY (SELECT n FROM subquery_predicate_values WHERE false)",
            vec![vec![Some("f")]],
            16,
        ),
        (
            "SELECT 150 = ALL (SELECT n FROM subquery_predicate_values WHERE false)",
            vec![vec![Some("t")]],
            16,
        ),
        (
            "SELECT outer_row.id FROM subquery_predicate_values AS outer_row \
             WHERE outer_row.n IN (SELECT inner_row.n \
                 FROM subquery_predicate_values AS inner_row \
                 WHERE inner_row.id = outer_row.id) ORDER BY outer_row.id",
            vec![vec![Some("1")], vec![Some("2")]],
            23,
        ),
    ];
    let mut steps = vec![predicate_setup()];
    steps.extend(
        cases
            .iter()
            .map(|(sql, _, _)| Step::ThroughReady(vec![wire::simple_query_frame(sql)])),
    );

    // Act
    let batches = run_script("issue-762-subquery-predicate-truth-tables", steps);
    let mut mismatches = Vec::new();
    for (index, (sql, expected_rows, expected_oid)) in cases.iter().enumerate() {
        let batch = &batches[index + 1];
        if let Some(state) = wire::error_code(batch) {
            mismatches.push(format!(
                "{sql}: unexpected SQLSTATE {state} ({})",
                error_summary(batch).unwrap_or_else(|| "missing ErrorResponse".into())
            ));
            continue;
        }

        let expected_rows = expected_rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|value| value.map(str::to_owned))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let actual_rows = wire::data_rows(batch);
        if actual_rows != expected_rows {
            mismatches.push(format!(
                "{sql}: expected {expected_rows:?}, got {actual_rows:?}"
            ));
        }

        match batch.iter().find(|frame| frame.0 == b'T') {
            Some(description) => {
                let columns = wire::parse_row_description(&description.1);
                if columns.len() != 1 || columns[0].type_oid != *expected_oid {
                    mismatches.push(format!(
                        "{sql}: expected one column with OID {expected_oid}, got {columns:?}"
                    ));
                }
            }
            None => mismatches.push(format!("{sql}: missing RowDescription")),
        }
    }

    // Assert
    assert!(
        mismatches.is_empty(),
        "PostgreSQL 18.6 subquery predicate results differ:\n{}",
        mismatches.join("\n")
    );
}

#[test]
fn should_report_wide_subquery_predicate_arity_errors() {
    // Arrange
    let cases = [
        "SELECT 1 IN (SELECT 1, 2)",
        "SELECT 1 = ANY (SELECT 1, 2)",
        "SELECT 1 = ALL (SELECT 1, 2)",
    ];
    let mut steps = Vec::with_capacity(cases.len());
    steps.extend(
        cases
            .iter()
            .map(|sql| Step::ThroughReady(vec![wire::simple_query_frame(sql)])),
    );

    // Act
    let batches = run_script("issue-762-wide-predicate-subqueries", steps);
    let mismatches = cases
        .iter()
        .zip(&batches)
        .map(|(sql, batch)| match batch.iter().find(|frame| frame.0 == b'E') {
            Some(error) => {
                let fields = wire::parse_error_fields(&error.1);
                let state = fields
                    .iter()
                    .find(|(kind, _)| *kind == 'C')
                    .map(|(_, value)| value.as_str());
                let message = fields
                    .iter()
                    .find(|(kind, _)| *kind == 'M')
                    .map(|(_, value)| value.as_str());
                (state != Some("42601")
                    || message != Some("subquery has too many columns"))
                    .then(|| {
                        format!(
                            "{sql}: expected 42601 subquery has too many columns, got {state:?} {message:?}"
                        )
                    })
            }
            None => Some(format!("{sql}: expected SQLSTATE 42601, got no ErrorResponse")),
        })
        .flatten()
        .collect::<Vec<_>>();

    // Assert
    assert_eq!(
        batches.len(),
        cases.len(),
        "each malformed predicate statement must be processed"
    );
    assert!(
        mismatches.is_empty(),
        "wide predicate subquery errors differ:\n{}",
        mismatches.join("\n")
    );
}

#[test]
fn should_bind_subquery_predicate_parameters_across_formats() {
    // Arrange
    let binary_value = 100_i32.to_be_bytes();
    let batches = run_script(
        "issue-762-subquery-predicate-parameter-wire",
        vec![
            predicate_setup(),
            Step::ParseFlush(wire::parse_frame_with_types(
                "predicate",
                "SELECT $1 IN (SELECT n FROM subquery_predicate_values) AS matched",
                &[0],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("predicate"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame("predicate_text", "predicate", &["100"]),
                wire::execute_frame("predicate_text"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    "predicate_binary",
                    "predicate",
                    &[1],
                    &[Some(&binary_value)],
                    &[1],
                ),
                wire::describe_portal_frame("predicate_binary"),
                wire::execute_frame("predicate_binary"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let parsed = &batches[1];
    let described = &batches[2];
    let text_result = &batches[3];
    let binary_result = &batches[4];

    // Assert
    assert_eq!(
        wire::error_code(parsed),
        None,
        "Parse ErrorResponse: {:?}",
        error_summary(parsed)
    );
    assert_eq!(
        wire::error_code(described),
        None,
        "Describe ErrorResponse: {:?}",
        error_summary(described)
    );
    let parameter_description = described
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("parameter description for the inferred integer input");
    assert_eq!(
        wire::parse_parameter_description(&parameter_description.1),
        vec![23]
    );
    let statement_description = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("Boolean result description");
    assert_eq!(
        wire::parse_row_description(&statement_description.1)[0].type_oid,
        16
    );
    assert_eq!(wire::error_code(text_result), None);
    assert!(!text_result.iter().any(|frame| frame.0 == b'T'));
    assert_eq!(wire::data_rows(text_result), vec![vec![Some("t".into())]]);
    assert_eq!(wire::error_code(binary_result), None);
    let binary_description = binary_result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("binary Boolean portal description");
    let binary_column = &wire::parse_row_description(&binary_description.1)[0];
    assert_eq!(binary_column.type_oid, 16);
    assert_eq!(binary_column.format_code, 1);
    let binary_row = binary_result
        .iter()
        .find(|frame| frame.0 == b'D')
        .expect("binary Boolean result row");
    assert_eq!(
        i16::from_be_bytes(binary_row.1[0..2].try_into().unwrap()),
        1
    );
    assert_eq!(
        i32::from_be_bytes(binary_row.1[2..6].try_into().unwrap()),
        1
    );
    assert_eq!(binary_row.1[6], 1);
}
