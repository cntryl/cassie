//! PostgreSQL 18 pattern and text-search protocol contracts tracked by #799.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

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
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 'text' ~ '('",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result).as_deref(), Some("2201B"));
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
fn should_bind_unicode_regex_under_icu_collation() {
    // Arrange
    let sql = "SELECT $1 COLLATE \"und-x-icu\" ~* $2 AS matched";
    let batches = run_script(
        "issue-799-regex-unicode-icu-wire",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "unicode_regex_parameters",
                sql,
                &[25, 25],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("unicode_regex_parameters"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame_with_formats(
                    "unicode_regex_portal",
                    "unicode_regex_parameters",
                    &[0],
                    &[Some("café".as_bytes()), Some("^CAFÉ$".as_bytes())],
                    &[1],
                ),
                wire::describe_portal_frame("unicode_regex_portal"),
                wire::execute_frame("unicode_regex_portal"),
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
                .expect("Unicode regex ParameterDescription")
                .1
        ),
        vec![25, 25]
    );
    let column = wire::parse_row_description(
        &executed
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("Unicode regex portal RowDescription")
            .1,
    )
    .remove(0);
    assert_eq!(column.type_oid, 16);
    assert_eq!(column.format_code, 1);
    assert_eq!(
        executed
            .iter()
            .find(|(tag, _)| *tag == b'D')
            .expect("binary Unicode regex Boolean DataRow")
            .1,
        vec![0, 1, 0, 0, 0, 1, 1]
    );
}

#[test]
fn should_bind_regconfig_for_tsvector_with_postgres_metadata() {
    // Arrange
    let sql = "SELECT to_tsvector($1, $2) AS vector";
    let batches = run_script(
        "issue-799-regconfig-binary-protocol",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "regconfig_parameters",
                sql,
                &[3734, 25],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("regconfig_parameters"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame(
                    "regconfig_portal",
                    "regconfig_parameters",
                    &["english", "The Fat Rats"],
                ),
                wire::describe_portal_frame("regconfig_portal"),
                wire::execute_frame("regconfig_portal"),
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
                .expect("regconfig ParameterDescription")
                .1
        ),
        vec![3734, 25]
    );
    let column = wire::parse_row_description(
        &executed
            .iter()
            .find(|(tag, _)| *tag == b'T')
            .expect("tsvector portal RowDescription")
            .1,
    )
    .remove(0);
    assert_eq!(column.type_oid, 3614);
    assert_eq!(column.format_code, 0);
    assert_eq!(
        wire::data_rows(executed),
        vec![vec![Some("'fat':2 'rat':3".to_owned())]]
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
fn should_describe_tsvector_results_with_the_postgres_oid() {
    // Arrange
    let batches = run_script(
        "issue-799-tsvector-type-wire",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "tsvector_parameter",
                "SELECT to_tsvector('simple', $1) AS value",
                &[25],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("tsvector_parameter"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let described = &batches[1];

    // Assert
    assert_eq!(wire::error_code(described), None);
    let columns = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("tsvector RowDescription");
    assert_eq!(wire::parse_row_description(&columns.1)[0].type_oid, 3614);
}

#[test]
fn should_describe_tsquery_results_with_the_postgres_oid() {
    // Arrange
    let batches = run_script(
        "issue-799-tsquery-type-wire",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "tsquery_parameter",
                "SELECT to_tsquery('simple', $1) AS value",
                &[25],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("tsquery_parameter"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let described = &batches[1];

    // Assert
    assert_eq!(wire::error_code(described), None);
    let columns = described
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("tsquery RowDescription");
    assert_eq!(wire::parse_row_description(&columns.1)[0].type_oid, 3615);
}

#[test]
fn should_bind_text_patterns_to_tsquery_constructors() {
    // Arrange
    let batches = run_script(
        "issue-799-tsquery-parameter-bind",
        vec![
            Step::ParseFlush(wire::parse_frame_with_types(
                "tsquery_parameter",
                "SELECT to_tsquery('simple', $1) AS value",
                &[25],
            )),
            Step::ThroughReady(vec![
                wire::describe_statement_frame("tsquery_parameter"),
                wire::sync_frame(),
            ]),
            Step::ThroughReady(vec![
                wire::bind_frame("tsquery_portal", "tsquery_parameter", &["blue & sky"]),
                wire::execute_frame("tsquery_portal"),
                wire::sync_frame(),
            ]),
        ],
    );

    // Act
    let result = &batches[2];

    // Assert
    assert_eq!(wire::error_code(result), None);
    let columns = batches[1]
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("statement Describe RowDescription");
    assert_eq!(wire::parse_row_description(&columns.1)[0].type_oid, 3615);
    assert!(
        !result.iter().any(|frame| frame.0 == b'T'),
        "Execute without portal Describe must not send another RowDescription"
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("'blue' & 'sky'".to_owned())]]
    );
}

#[test]
fn should_return_unique_violation_for_duplicate_text_search_mapping() {
    // Arrange
    let batches = run_script(
        "issue-799-duplicate-text-search-mapping",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TEXT SEARCH CONFIGURATION issue_799_duplicate (COPY = pg_catalog.english)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "ALTER TEXT SEARCH CONFIGURATION issue_799_duplicate ADD MAPPING FOR asciiword WITH pg_catalog.english_stem",
            )]),
        ],
    );

    // Act
    let created = &batches[0];
    let duplicate_mapping = &batches[1];

    // Assert
    assert_eq!(wire::error_code(created), None);
    assert_eq!(
        wire::error_code(duplicate_mapping).as_deref(),
        Some("23505")
    );
}

#[test]
fn should_report_missing_text_search_parser_callback_with_42883() {
    // Arrange
    let batches = run_script(
        "issue-799-missing-parser-callback",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TEXT SEARCH PARSER issue_799_parser (START = issue_start, GETTOKEN = issue_next, END = issue_end, LEXTYPES = issue_lex_types)",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result).as_deref(), Some("42883"));
}

#[test]
fn should_report_missing_text_search_template_callback_with_42883() {
    // Arrange
    let batches = run_script(
        "issue-799-missing-template-callback",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE TEXT SEARCH TEMPLATE issue_799_template (INIT = issue_init, LEXIZE = issue_lexize)",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result).as_deref(), Some("42883"));
}
