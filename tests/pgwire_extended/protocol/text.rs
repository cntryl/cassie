//! PostgreSQL 18 pattern and text-search protocol contracts tracked by #799.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[path = "../patterns/like_escape/nulls.rs"]
mod like_escape_residuals;
#[path = "../patterns/named_regexp_like_parameters.rs"]
mod named_regexp_like_parameters;

#[test]
fn should_describe_ilike_parameter_types() {
    // Arrange
    let batches = run_script(
        "issue-799-ilike-parameter-description",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "ilike_parameters",
                "SELECT $1 ILIKE $2 AS matched",
                &[25, 25],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("ilike_parameters"),
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
    assert_eq!(
        wire::parse_parameter_description(&parameters.1),
        vec![25, 25]
    );
    let columns = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("RowDescription");
    assert_eq!(wire::parse_row_description(&columns.1)[0].type_oid, 16);
}

#[test]
fn should_bind_text_values_to_ilike_operands() {
    // Arrange
    let batches = run_script(
        "issue-799-ilike-parameter-bind",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "ilike_parameters",
                "SELECT $1 ILIKE $2 AS matched",
                &[25, 25],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("ilike_parameters"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame("ilike_portal", "ilike_parameters", &["Alice", "a%"]),
                wire::execute_frame("ilike_portal"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let parsed = &batches[0];
    let result = &batches[2];

    // Assert
    assert_eq!(
        parsed.iter().map(|frame| frame.0).collect::<Vec<_>>(),
        vec![b'1']
    );
    assert_eq!(wire::error_code(result), None);
    assert_eq!(wire::data_rows(result), vec![vec![Some("t".to_owned())]]);
}

#[test]
fn should_describe_regex_text_parameters_with_boolean_result_metadata() {
    // Arrange
    let batches = run_script(
        "issue-799-regex-parameter-description",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "regex_parameters",
                "SELECT $1 ~ $2 AS matched",
                &[25, 25],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("regex_parameters"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let described = &batches[1];

    // Assert
    assert_eq!(wire::error_code(described), None);
    let parameters = described
        .iter()
        .find(|frame| frame.0 == b't')
        .expect("ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameters.1),
        vec![25, 25]
    );
    let columns = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("RowDescription");
    assert_eq!(wire::parse_row_description(&columns.1)[0].type_oid, 16);
}

#[test]
fn should_return_postgres_invalid_regex_sqlstate_on_the_wire() {
    // Arrange
    let batches = run_script(
        "issue-799-invalid-regex-wire",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT 'text' ~ 'text' AS valid_pattern",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT 'text' ~ '(' AS malformed_pattern",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT 45 AS recovery")]),
        ],
    );

    // Act
    let valid = &batches[0];
    let invalid = &batches[1];
    let recovery = &batches[2];

    // Assert
    assert_eq!(wire::error_code(valid), None);
    assert_eq!(wire::data_rows(valid), vec![vec![Some("t".to_owned())]]);
    assert_invalid_pattern_error(invalid, "regex operator");
    assert_eq!(wire::error_code(recovery), None);
    assert_eq!(wire::data_rows(recovery), vec![vec![Some("45".to_owned())]]);
}

#[test]
fn should_bind_case_insensitive_regex_to_boolean_binary_result() {
    // Arrange
    let sql = "SELECT $1 ~* $2 AS matched";
    let batches = run_script(
        "issue-799-regex-binary-bind",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "regex_binary_parameters",
                sql,
                &[25, 25],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("regex_binary_parameters"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    "regex_binary_portal",
                    "regex_binary_parameters",
                    &[0],
                    &[Some(b"ALPHA"), Some(b"^alpha$")],
                    &[1],
                ),
                wire::describe_portal_frame("regex_binary_portal"),
                wire::execute_frame("regex_binary_portal"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let described = &batches[1];
    let executed = &batches[2];

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(described), None);
    assert_eq!(wire::error_code(executed), None);
    assert_eq!(
        wire::parse_parameter_description(
            &described
                .iter()
                .find(|(tag, _)| *tag == b't')
                .expect("regex ParameterDescription")
                .1
        ),
        vec![25, 25]
    );
    let column = wire::parse_row_description(
        &executed
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("regex portal RowDescription")
            .1,
    )
    .remove(0);
    assert_eq!(column.type_oid, 16);
    assert_eq!(column.format_code, 1);
    assert_eq!(
        executed
            .iter()
            .find(|(tag, _)| *tag == b'D')
            .expect("binary Boolean DataRow")
            .1,
        vec![0, 1, 0, 0, 0, 1, 1]
    );
}

#[test]
fn should_bound_global_regexp_replace_output_expansion() {
    // Arrange
    let input = "a".repeat(16 * 1024);
    let expected = "bb".repeat(16 * 1024);
    let sql = "SELECT regexp_replace($1, 'a', 'bb', 'g') AS expanded";
    let batches = run_script(
        "issue-799-regexp-replace-bounded-expansion",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "regexp_replace_input",
                sql,
                &[25],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("regexp_replace_input"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame("regexp_replace_portal", "regexp_replace_input", &[&input]),
                wire::describe_portal_frame("regexp_replace_portal"),
                wire::execute_frame("regexp_replace_portal"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let described = &batches[1];
    let executed = &batches[2];

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(described), None);
    assert_eq!(wire::error_code(executed), None);
    let column = wire::parse_row_description(
        &executed
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("regexp_replace portal RowDescription")
            .1,
    )
    .remove(0);
    assert_eq!(column.type_oid, 25);
    assert_eq!(column.format_code, 0);
    assert_eq!(wire::data_rows(executed), vec![vec![Some(expected)]]);
}

#[test]
fn should_reject_valid_c_backed_text_search_parser_with_0a000() {
    // PostgreSQL 18.6 accepts these exact built-in C callbacks. Cassie
    // excludes native C/plugin ABIs, so this valid form must reach a stable
    // feature-not-supported rejection rather than fail callback lookup.
    // Arrange
    let batches = run_script(
        "issue-799-c-backed-text-search-parser-rejection",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TEXT SEARCH PARSER issue_799_c_parser ( \
                    START = pg_catalog.prsd_start, \
                    GETTOKEN = pg_catalog.prsd_nexttoken, \
                    END = pg_catalog.prsd_end, \
                    LEXTYPES = pg_catalog.prsd_lextype \
                )",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT 45 AS recovery")]),
        ],
    );

    // Act
    let parser_error = &batches[0];
    let recovery = &batches[1];

    // Assert
    assert_eq!(wire::error_code(parser_error).as_deref(), Some("0A000"));
    assert_eq!(wire::error_code(recovery), None);
    assert_eq!(wire::data_rows(recovery), vec![vec![Some("45".to_owned())]]);
}

#[test]
fn should_reject_valid_c_backed_text_search_template_with_0a000() {
    // PostgreSQL 18.6 accepts these exact built-in C callbacks. Cassie
    // excludes native C/plugin ABIs, so this valid form must reach a stable
    // feature-not-supported rejection rather than fail callback lookup.
    // Arrange
    let batches = run_script(
        "issue-799-c-backed-text-search-template-rejection",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TEXT SEARCH TEMPLATE issue_799_c_template ( \
                    INIT = pg_catalog.dsimple_init, \
                    LEXIZE = pg_catalog.dsimple_lexize \
                )",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT 45 AS recovery")]),
        ],
    );

    // Act
    let template_error = &batches[0];
    let recovery = &batches[1];

    // Assert
    assert_eq!(wire::error_code(template_error).as_deref(), Some("0A000"));
    assert_eq!(wire::error_code(recovery), None);
    assert_eq!(wire::data_rows(recovery), vec![vec![Some("45".to_owned())]]);
}

#[test]
fn should_report_invalid_regular_expression_function_sqlstate() {
    // Arrange
    let batches = run_script(
        "issue-799-invalid-regex-function-sqlstate",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT regexp_replace('x', 'x', 'y', 'g') AS valid_pattern",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT regexp_replace('x', '[', '', 'g') AS malformed_function_pattern",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame("SELECT 45 AS recovery")]),
        ],
    );

    // Act
    let valid = &batches[0];
    let function_error = &batches[1];
    let recovery = &batches[2];

    // Assert
    assert_eq!(wire::error_code(valid), None);
    assert_eq!(wire::data_rows(valid), vec![vec![Some("y".to_owned())]]);
    assert_invalid_pattern_error(function_error, "regexp_replace");
    assert_eq!(wire::error_code(recovery), None);
    assert_eq!(wire::data_rows(recovery), vec![vec![Some("45".to_owned())]]);
}

fn assert_invalid_pattern_error(frames: &[(u8, Vec<u8>)], operation: &str) {
    assert_eq!(
        wire::error_code(frames).as_deref(),
        Some("2201B"),
        "{operation} SQLSTATE; frames: {frames:?}"
    );
    let fields = frames
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .map(|(_, payload)| wire::parse_error_fields(payload))
        .expect("invalid regex ErrorResponse");
    assert!(
        fields.iter().any(|(kind, value)| {
            *kind == 'M' && value.starts_with("invalid regular expression:")
        }),
        "{operation} must reach PostgreSQL's invalid-pattern error path: {fields:?}"
    );
    assert_eq!(wire::data_rows(frames), Vec::<Vec<Option<String>>>::new());
}
