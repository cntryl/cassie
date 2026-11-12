//! PostgreSQL 18.6 errors, SQLSTATEs and failure atomicity for the JSON element sources.

use super::json_elements_support::{docs_setup, error_message, query, text_rows};
use super::{fixture, wire};
use cassie::types::Value;

#[test]
fn should_reject_wrong_document_shapes_with_invalid_parameter_value_given_each_function_family() {
    // Arrange
    let cases = [
        (
            "SELECT * FROM jsonb_array_elements('3'::jsonb)",
            "cannot extract elements from a scalar",
        ),
        (
            "SELECT * FROM jsonb_array_elements('{\"a\":1}'::jsonb)",
            "cannot extract elements from an object",
        ),
        (
            "SELECT * FROM jsonb_array_elements_text('3'::jsonb)",
            "cannot extract elements from a scalar",
        ),
        (
            "SELECT * FROM json_array_elements('3'::json)",
            "cannot call json_array_elements on a scalar",
        ),
        (
            "SELECT * FROM json_array_elements('{\"a\":1}'::json)",
            "cannot call json_array_elements on a non-array",
        ),
        (
            "SELECT * FROM json_array_elements_text('{}'::json)",
            "cannot call json_array_elements_text on a non-array",
        ),
        (
            "SELECT * FROM jsonb_each('[1]'::jsonb)",
            "cannot call jsonb_each on a non-object",
        ),
        (
            "SELECT * FROM jsonb_each_text('3'::jsonb)",
            "cannot call jsonb_each_text on a non-object",
        ),
        (
            "SELECT * FROM json_each('[1]'::json)",
            "cannot deconstruct an array as an object",
        ),
        (
            "SELECT * FROM json_each_text('3'::json)",
            "cannot deconstruct a scalar",
        ),
    ];
    for (sql, message) in cases {
        // Act
        let frames = query("json-elements-shape-errors", &[], sql, false);
        // Assert
        assert_eq!(wire::error_code(&frames).as_deref(), Some("22023"), "{sql}");
        assert_eq!(error_message(&frames).as_deref(), Some(message), "{sql}");
        assert_eq!(wire::data_rows(&frames), Vec::<Vec<Option<String>>>::new());
    }
}

#[test]
fn should_reject_malformed_document_text_with_invalid_text_representation() {
    // Arrange
    let cases = [
        "SELECT * FROM jsonb_array_elements('[1,')",
        "SELECT * FROM json_array_elements('[1,')",
        "SELECT * FROM jsonb_each('xx')",
    ];
    for sql in cases {
        // Act
        let frames = query("json-elements-malformed-text", &[], sql, false);
        // Assert
        assert_eq!(wire::error_code(&frames).as_deref(), Some("22P02"), "{sql}");
        assert_eq!(wire::data_rows(&frames), Vec::<Vec<Option<String>>>::new());
    }
}

#[test]
fn should_reject_undefined_signatures_with_undefined_function() {
    // Arrange
    let cases = [
        "SELECT * FROM jsonb_array_elements(1)",
        "SELECT * FROM json_array_elements('[1]'::jsonb)",
        "SELECT * FROM jsonb_each('{}'::json)",
        "SELECT * FROM jsonb_array_elements('[1]'::jsonb, '[2]'::jsonb)",
        "SELECT * FROM json_each_text()",
    ];
    for sql in cases {
        // Act
        let frames = query("json-elements-undefined-signature", &[], sql, false);
        // Assert
        assert_eq!(wire::error_code(&frames).as_deref(), Some("42883"), "{sql}");
        assert_eq!(wire::data_rows(&frames), Vec::<Vec<Option<String>>>::new());
    }
}

#[test]
fn should_reject_nul_escape_with_unsupported_unicode_escape_given_json_each_text() {
    // Arrange
    let sql = r#"SELECT * FROM json_each_text('{"a":"\u0000"}'::json)"#;
    // Act
    let frames = query("json-elements-nul-escape", &[], sql, false);
    // Assert
    assert_eq!(wire::error_code(&frames).as_deref(), Some("22P05"));
    assert_eq!(wire::data_rows(&frames), Vec::<Vec<Option<String>>>::new());
}

#[test]
fn should_publish_no_partial_rows_given_a_later_lateral_input_with_the_wrong_shape() {
    // Arrange
    let mut setup = docs_setup();
    setup.push((
        "INSERT INTO json_elements_docs VALUES (4, '{\"a\":1}')",
        Vec::<Value>::new(),
    ));
    let failing = "SELECT t.id, e.value FROM json_elements_docs t, jsonb_array_elements(t.doc) e ORDER BY t.id, e.value";
    let recovery = "SELECT t.id, e.value FROM json_elements_docs t, jsonb_array_elements(t.doc) e WHERE t.id < 3 ORDER BY t.id, e.value";
    let cycles = vec![
        fixture::execute_cycle(failing, None, 0, false),
        fixture::execute_cycle(recovery, None, 0, false),
    ];
    // Act
    let batches = fixture::run_cycles("json-elements-atomic-failure", &setup, cycles);
    // Assert
    assert_eq!(
        wire::error_code(&batches[0]).as_deref(),
        Some("22023"),
        "the object document must fail the whole statement"
    );
    assert_eq!(
        wire::data_rows(&batches[0]),
        Vec::<Vec<Option<String>>>::new()
    );
    fixture::assert_success(&batches[1]);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![
            vec![Some("1".to_owned()), Some("1".to_owned())],
            vec![Some("1".to_owned()), Some("2".to_owned())],
        ]
    );
}

#[test]
fn should_keep_the_connection_usable_given_an_expansion_error() {
    // Arrange
    let cycles = vec![
        fixture::execute_cycle("SELECT * FROM jsonb_each('[1]'::jsonb)", None, 0, false),
        fixture::execute_cycle(
            "SELECT value FROM jsonb_array_elements('[7]'::jsonb)",
            None,
            0,
            false,
        ),
    ];
    // Act
    let batches = fixture::run_cycles("json-elements-error-recovery", &[], cycles);
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22023"));
    fixture::assert_success(&batches[1]);
    assert_eq!(wire::data_rows(&batches[1]), text_rows(&["7"]));
}

#[test]
fn should_reject_nul_escape_with_unsupported_unicode_escape_given_uncast_jsonb_text() {
    // Arrange
    let cases = [
        r#"SELECT * FROM jsonb_array_elements('["\u0000"]')"#,
        r#"SELECT * FROM jsonb_array_elements_text('["\u0000"]')"#,
        r#"SELECT * FROM jsonb_each('{"a":"\u0000"}')"#,
        r#"SELECT * FROM jsonb_each_text('{"a":"\u0000"}')"#,
    ];
    for sql in cases {
        // Act
        let frames = query("json-elements-jsonb-nul", &[], sql, false);
        // Assert
        assert_eq!(wire::error_code(&frames).as_deref(), Some("22P05"), "{sql}");
        assert_eq!(wire::data_rows(&frames), Vec::<Vec<Option<String>>>::new());
    }
}

#[test]
fn should_return_a_depth_error_given_nesting_beyond_the_json_parser_bound() {
    // Arrange
    let nested = format!("{}1{}", "[".repeat(200), "]".repeat(200));
    let array = format!("SELECT * FROM json_array_elements('[{nested}]')");
    let each = format!("SELECT * FROM json_each('{{\"a\":{nested}}}')");
    for sql in [array, each] {
        // Act
        let frames = query("json-elements-depth", &[], &sql, false);
        // Assert
        assert_eq!(wire::error_code(&frames).as_deref(), Some("54001"), "{sql}");
        assert_eq!(wire::data_rows(&frames), Vec::<Vec<Option<String>>>::new());
    }
}

#[test]
fn should_reject_cross_family_columns_with_undefined_function() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE json_elements_families (j JSON, b JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO json_elements_families VALUES ('{\"a\":1}', '{\"a\":1}')",
            Vec::new(),
        ),
    ];
    let cases = [
        "SELECT * FROM json_elements_families f, json_each(f.b) e",
        "SELECT * FROM json_elements_families f, jsonb_each(f.j) e",
        "SELECT * FROM json_elements_families f, json_array_elements(f.b) e",
    ];
    for sql in cases {
        // Act
        let frames = query("json-elements-cross-family", &setup, sql, false);
        // Assert
        assert_eq!(wire::error_code(&frames).as_deref(), Some("42883"), "{sql}");
    }
}

#[test]
fn should_return_the_documented_unsupported_error_given_select_list_use() {
    // Arrange
    let sql = "SELECT jsonb_array_elements('[1]'::jsonb)";
    // Act
    let frames = query("json-elements-select-list", &[], sql, false);
    // Assert
    assert_eq!(wire::error_code(&frames).as_deref(), Some("0A000"));
    assert_eq!(wire::data_rows(&frames), Vec::<Vec<Option<String>>>::new());
}

#[test]
fn should_filter_outer_rows_before_expanding_given_cross_join_lateral_with_an_erroring_row() {
    // Arrange
    let setup = [
        ("CREATE TABLE lateral_steps (id INT, step INT)", Vec::new()),
        (
            "INSERT INTO lateral_steps VALUES (1, 1), (2, 1), (3, 0)",
            Vec::new(),
        ),
    ];
    let unfiltered =
        "SELECT t.id, g FROM lateral_steps t CROSS JOIN LATERAL generate_series(1, 2, t.step) g";
    let filtered = "SELECT t.id, g FROM lateral_steps t CROSS JOIN LATERAL generate_series(1, 2, t.step) g WHERE t.id < 3 ORDER BY t.id, g";
    let output_conjunct = "SELECT t.id, g FROM lateral_steps t CROSS JOIN LATERAL generate_series(1, 2, t.step) g WHERE t.id < 3 AND g = 2 ORDER BY t.id";
    let cycles = vec![
        fixture::execute_cycle(unfiltered, None, 0, false),
        fixture::execute_cycle(filtered, None, 0, false),
        fixture::execute_cycle(output_conjunct, None, 0, false),
    ];
    // Act
    let batches = fixture::run_cycles("json-elements-outer-filter-pushdown", &setup, cycles);
    // Assert
    assert!(
        wire::error_code(&batches[0]).is_some(),
        "the zero step must fail when the outer row is not filtered"
    );
    fixture::assert_success(&batches[1]);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![
            vec![Some("1".to_owned()), Some("1".to_owned())],
            vec![Some("1".to_owned()), Some("2".to_owned())],
            vec![Some("2".to_owned()), Some("1".to_owned())],
            vec![Some("2".to_owned()), Some("2".to_owned())],
        ]
    );
    fixture::assert_success(&batches[2]);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![
            vec![Some("1".to_owned()), Some("2".to_owned())],
            vec![Some("2".to_owned()), Some("2".to_owned())],
        ]
    );
}

#[test]
fn should_keep_function_output_conjuncts_above_given_json_each_lateral() {
    // Arrange
    let mut setup = docs_setup();
    setup.push((
        "INSERT INTO json_elements_docs VALUES (4, '{\"a\":1}')",
        Vec::<Value>::new(),
    ));
    let sql = "SELECT t.id, e.value FROM json_elements_docs t, jsonb_array_elements(t.doc) e WHERE t.id < 3 AND e.value::text <> '1' ORDER BY t.id";
    // Act
    let frames = query("json-elements-output-conjunct", &setup, sql, false);
    // Assert
    fixture::assert_success(&frames);
    assert_eq!(
        wire::data_rows(&frames),
        vec![vec![Some("1".to_owned()), Some("2".to_owned())]]
    );
}
