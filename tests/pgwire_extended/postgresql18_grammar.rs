//! PostgreSQL 18 parser, relation, collation, and wire contracts tracked by #796 and #800.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_describe_postgres_cast_parameter_oid_through_extended_protocol() {
    // Arrange
    let batches = run_script(
        "issue-796-cast-parameter-wire",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "cast_parameter",
                "SELECT $1::DOUBLE PRECISION AS value",
                &[0],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("cast_parameter"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let parsed = &batches[0];
    let described = &batches[1];

    // Assert
    assert_eq!(
        parsed.iter().map(|frame| frame.0).collect::<Vec<_>>(),
        vec![b'1']
    );
    assert_eq!(wire::error_code(described), None);
    let parameter_types = described
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_types.1),
        vec![701]
    );
    let columns = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("RowDescription");
    assert_eq!(wire::parse_row_description(&columns.1)[0].type_oid, 701);
}

#[test]
fn should_report_postgres_collation_conflicts_with_sqlstate_42p21() {
    // Arrange
    let batches = run_script(
        "issue-796-collation-conflict-wire",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT ('a' COLLATE \"C\") < ('b' COLLATE \"POSIX\")",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result).as_deref(), Some("42P21"));
}

#[test]
fn should_expose_the_builtin_postgres_collation_catalog_rows() {
    // Arrange
    let batches = run_script(
        "issue-796-pg-collation-wire",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT collname, collprovider, collisdeterministic FROM pg_collation WHERE collname = 'C'",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    assert_eq!(
        wire::row_description_names(result),
        vec!["collname", "collprovider", "collisdeterministic"]
    );
    assert_eq!(wire::data_rows(result).len(), 1);
    assert_eq!(wire::data_rows(result)[0][0].as_deref(), Some("C"));
}

#[test]
fn should_describe_parenthesized_parameter_arithmetic_in_offset() {
    // Arrange
    let batches = run_script(
        "issue-800-parenthesized-offset-arithmetic",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "good_offset",
                "SELECT 1 OFFSET ($1 + 1) ROWS",
                &[23],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("good_offset"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let parsed = &batches[0];
    let described = &batches[1];

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
    let columns = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("RowDescription");
    assert_eq!(wire::parse_row_description(&columns.1)[0].type_oid, 23);
}

#[test]
fn should_report_malformed_unicode_identifier_escape_with_syntax_sqlstate() {
    // Arrange
    let batches = run_script(
        "issue-796-malformed-unicode-identifier",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT U&\"bad!00G0\" UESCAPE '!'",
        )])],
    );

    // Act
    let result = &batches[0];
    let error = result
        .iter()
        .find(|frame| frame.0 == b'E')
        .expect("malformed Unicode identifier ErrorResponse");
    let fields = wire::parse_error_fields(&error.1);

    // Assert
    assert_eq!(wire::error_code(result).as_deref(), Some("42601"));
    assert_eq!(
        fields
            .iter()
            .find(|(kind, _)| *kind == 'M')
            .map(|(_, value)| value.as_str()),
        Some("invalid Unicode escape")
    );
}

#[test]
fn should_report_unsupported_comparison_for_empty_rows_with_0a000() {
    // Arrange
    let batches = run_script(
        "issue-800-empty-row-equality-sqlstate",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT ROW() = ROW()",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result).as_deref(), Some("0A000"));
}

#[test]
fn should_compare_values_from_literal_row_constructors() {
    // Arrange
    let batches = run_script(
        "issue-800-literal-row-equality",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT ROW(1, 2) = ROW(1, 2)",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    assert_eq!(wire::data_rows(result), vec![vec![Some("t".to_owned())]]);
}

#[test]
fn should_report_undefined_operator_for_literal_row_image_comparisons() {
    // Arrange
    let operators = ["*=", "*<>", "*<", "*<=", "*>", "*>="];
    let steps = operators
        .iter()
        .map(|operator| {
            Step::ThroughReady(vec![wire::simple_query_frame(&format!(
                "SELECT ROW(1, 2) {operator} ROW(1, 2)"
            ))])
        })
        .collect();
    let batches = run_script("issue-800-row-image-operator-sqlstate", steps);

    // Act
    let errors = batches
        .iter()
        .map(|frames| wire::error_code(frames))
        .collect::<Vec<_>>();

    // Assert
    assert_eq!(errors, vec![Some("42883".to_owned()); operators.len()]);
}

#[test]
fn should_sample_rows_with_a_valid_bernoulli_percentage() {
    // Arrange
    let batches = run_script(
        "issue-800-tablesample-valid-rate",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_800_sample_valid_rows (id INT)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_800_sample_valid_rows VALUES (1)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT id FROM issue_800_sample_valid_rows TABLESAMPLE BERNOULLI (100)",
            )]),
        ],
    );

    // Act
    let created = &batches[0];
    let inserted = &batches[1];
    let sampled = &batches[2];

    // Assert
    assert_eq!(wire::error_code(created), None);
    assert_eq!(wire::error_code(inserted), None);
    assert_eq!(wire::error_code(sampled), None);
    assert_eq!(wire::data_rows(sampled), vec![vec![Some("1".to_owned())]]);
}

#[test]
fn should_report_invalid_table_sample_percentages_with_2202h() {
    // Arrange
    let batches = run_script(
        "issue-800-tablesample-range-sqlstate",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_800_sample_error_rows (id INT)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_800_sample_error_rows VALUES (1)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT id FROM issue_800_sample_error_rows TABLESAMPLE BERNOULLI (-1)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT id FROM issue_800_sample_error_rows TABLESAMPLE SYSTEM (101)",
            )]),
        ],
    );

    // Act
    let created = &batches[0];
    let inserted = &batches[1];
    let negative_rate = &batches[2];
    let overfull_rate = &batches[3];

    // Assert
    assert_eq!(wire::error_code(created), None);
    assert_eq!(wire::error_code(inserted), None);
    assert_eq!(wire::error_code(negative_rate).as_deref(), Some("2202H"));
    assert_eq!(wire::error_code(overfull_rate).as_deref(), Some("2202H"));
}

#[test]
fn should_report_values_using_row_arity_sqlstates() {
    // Arrange
    let cases = [
        ("VALUES (1, 2), (3)", "42601"),
        ("SELECT * FROM (VALUES (1, 2)) AS v(x, y, z)", "42P10"),
        ("SELECT * FROM (VALUES (1), (TRUE)) AS v(value)", "42804"),
        ("SELECT ROW(1, 2) = ROW(1)", "42601"),
        (
            "SELECT a.id FROM (VALUES (1)) AS a(id) JOIN (VALUES (1)) AS b(id) USING (missing)",
            "42703",
        ),
        (
            "SELECT * FROM (VALUES (1)) AS a(id) JOIN (VALUES (1)) AS b(id) USING (id, id)",
            "42701",
        ),
    ];
    let steps = cases
        .iter()
        .map(|(sql, _)| Step::ThroughReady(vec![wire::simple_query_frame(sql)]))
        .collect();

    // Act
    let batches = run_script("issue-800-values-using-row-errors", steps);
    let actual = batches
        .iter()
        .map(|batch| wire::error_code(batch))
        .collect::<Vec<_>>();
    let expected = cases
        .iter()
        .map(|(_, state)| Some((*state).to_owned()))
        .collect::<Vec<_>>();

    // Assert
    assert_eq!(actual, expected);
}
