//! PostgreSQL jsonpath item-method values and JSONB result identity.

use super::super::support_pgwire as wire;
use super::super::support_type_metadata_contract as fixture;

#[test]
fn should_apply_jsonpath_type_method_with_match_operator() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT '{\"items\":[1,2,3]}'::JSONB @@ '$.items.type() == \"array\"'::JSONPATH",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonpath-type-method-match", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONPath type method RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size))
            .collect::<Vec<_>>(),
        vec![(16, 1)]
    );
    assert_eq!(wire::data_rows(frames), vec![vec![Some("t".to_owned())]]);
}

#[test]
fn should_apply_jsonpath_size_method_with_match_operator() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT '{\"items\":[1,2,3]}'::JSONB @@ '$.items.size() == 3'::JSONPATH",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonpath-size-method-match", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONPath size method RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size))
            .collect::<Vec<_>>(),
        vec![(16, 1)]
    );
    assert_eq!(wire::data_rows(frames), vec![vec![Some("t".to_owned())]]);
}

#[test]
fn should_find_jsonpath_absolute_value_with_exists_operator() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT '{\"items\":[-1,2,-3]}'::JSONB @? '$.items[*] ? (@.abs() == 3)'::JSONPATH",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonpath-abs-method-exists", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONPath absolute value RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size))
            .collect::<Vec<_>>(),
        vec![(16, 1)]
    );
    assert_eq!(wire::data_rows(frames), vec![vec![Some("t".to_owned())]]);
}

#[test]
fn should_convert_boolean_item_with_jsonpath_boolean_method() {
    // Arrange
    let document = "'{\"enabled\":true}'::JSONB";
    let path = "$.enabled.boolean() == true";

    // Act
    let frames = run_jsonpath_match("issue-809-jsonpath-boolean-method", document, path);

    // Assert
    assert_jsonpath_match_true(&frames);
}

#[test]
fn should_convert_string_item_with_jsonpath_string_method() {
    // Arrange
    let document = "'{\"name\":\"Ada\"}'::JSONB";
    let path = "$.name.string() == \"Ada\"";

    // Act
    let frames = run_jsonpath_match("issue-809-jsonpath-string-method", document, path);

    // Assert
    assert_jsonpath_match_true(&frames);
}

#[test]
fn should_convert_numeric_item_with_jsonpath_double_method() {
    // Arrange
    let document = "'{\"fraction\":12.5}'::JSONB";
    let path = "$.fraction.double() == 12.5";

    // Act
    let frames = run_jsonpath_match("issue-809-jsonpath-double-method", document, path);

    // Assert
    assert_jsonpath_match_true(&frames);
}

#[test]
fn should_round_numeric_item_with_jsonpath_ceiling_method() {
    // Arrange
    let document = "'{\"fraction\":12.5}'::JSONB";
    let path = "$.fraction.ceiling() == 13";

    // Act
    let frames = run_jsonpath_match("issue-809-jsonpath-ceiling-method", document, path);

    // Assert
    assert_jsonpath_match_true(&frames);
}

#[test]
fn should_round_numeric_item_with_jsonpath_floor_method() {
    // Arrange
    let document = "'{\"fraction\":12.5}'::JSONB";
    let path = "$.fraction.floor() == 12";

    // Act
    let frames = run_jsonpath_match("issue-809-jsonpath-floor-method", document, path);

    // Assert
    assert_jsonpath_match_true(&frames);
}

#[test]
fn should_convert_integer_item_with_jsonpath_bigint_method() {
    // Arrange
    let document = "'{\"whole\":42}'::JSONB";
    let path = "$.whole.bigint() == 42";

    // Act
    let frames = run_jsonpath_match("issue-809-jsonpath-bigint-method", document, path);

    // Assert
    assert_jsonpath_match_true(&frames);
}

#[test]
fn should_convert_numeric_item_with_jsonpath_decimal_method() {
    // Arrange
    let document = "'{\"fraction\":12.5}'::JSONB";
    let path = "$.fraction.decimal(5,2) == 12.50";

    // Act
    let frames = run_jsonpath_match("issue-809-jsonpath-decimal-method", document, path);

    // Assert
    assert_jsonpath_match_true(&frames);
}

#[test]
fn should_convert_integer_item_with_jsonpath_integer_method() {
    // Arrange
    let document = "'{\"whole\":42}'::JSONB";
    let path = "$.whole.integer() == 42";

    // Act
    let frames = run_jsonpath_match("issue-809-jsonpath-integer-method", document, path);

    // Assert
    assert_jsonpath_match_true(&frames);
}

#[test]
fn should_convert_numeric_item_with_jsonpath_number_method() {
    // Arrange
    let document = "'{\"fraction\":12.5}'::JSONB";
    let path = "$.fraction.number() == 12.5";

    // Act
    let frames = run_jsonpath_match("issue-809-jsonpath-number-method", document, path);

    // Assert
    assert_jsonpath_match_true(&frames);
}

#[test]
fn should_reject_invalid_jsonpath_match_result_shape() {
    // Arrange
    let cases = [
        "SELECT jsonb_path_match('{\"a\":1}'::JSONB, '$.a'::JSONPATH)",
        "SELECT jsonb_path_match('[1,2]'::JSONB, '$[*]'::JSONPATH)",
        "SELECT jsonb_path_match('[true,false]'::JSONB, '$[*]'::JSONPATH)",
        "SELECT jsonb_path_match('{\"a\":1}'::JSONB, '$.missing'::JSONPATH)",
    ];
    let cycles = cases
        .iter()
        .map(|sql| fixture::execute_cycle(sql, None, 0, true))
        .collect();

    // Act
    let batches = fixture::run_cycles("issue-1467-jsonpath-match-cardinality", &[], cycles);

    // Assert
    assert_eq!(batches.len(), cases.len());
    for (sql, frames) in cases.iter().zip(&batches) {
        assert_eq!(wire::error_code(frames).as_deref(), Some("22038"), "{sql}");
        let message = frames
            .iter()
            .find(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .expect("JSONPath match error fields")
            .into_iter()
            .find_map(|(code, message)| (code == 'M').then_some(message))
            .expect("JSONPath match error message");
        assert_eq!(message, "single boolean result is expected", "{sql}");
        assert!(wire::data_rows(frames).is_empty(), "{sql}");
    }
}

#[test]
fn should_return_unknown_for_non_boolean_jsonpath_match_operator_results() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT '{\"a\":1}'::JSONB @@ '$.a'::JSONPATH, '{\"a\":1}'::JSONB @@ '$.missing'::JSONPATH, '[1,2]'::JSONB @@ '$[*]'::JSONPATH",
        None,
        0,
        true,
    );

    // Act
    let frames = fixture::run_cycles("issue-1467-jsonpath-match-operator-unknown", &[], vec![cycle])
        .remove(0);

    // Assert
    fixture::assert_success(&frames);
    assert_eq!(
        wire::data_rows(&frames),
        vec![vec![None, None, None]]
    );
}

#[test]
fn should_return_true_for_boolean_jsonpath_match_operator_result() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT '{\"a\":true}'::JSONB @@ '$.a'::JSONPATH",
        None,
        0,
        true,
    );

    // Act
    let frames = fixture::run_cycles("issue-1467-jsonpath-match-operator-boolean", &[], vec![cycle])
        .remove(0);

    // Assert
    fixture::assert_success(&frames);
    assert_eq!(wire::data_rows(&frames), vec![vec![Some("t".to_owned())]]);
}

#[test]
fn should_preserve_true_jsonpath_match_result() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_path_match('{\"a\":true}'::JSONB, '$.a'::JSONPATH)",
        None,
        0,
        true,
    );

    // Act
    let frames = fixture::run_cycles("issue-1467-jsonpath-match-true", &[], vec![cycle])
        .remove(0);

    // Assert
    fixture::assert_success(&frames);
    assert_eq!(wire::data_rows(&frames), vec![vec![Some("t".to_owned())]]);
}

#[test]
fn should_preserve_false_jsonpath_match_result() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_path_match('{\"a\":false}'::JSONB, '$.a'::JSONPATH)",
        None,
        0,
        true,
    );

    // Act
    let frames = fixture::run_cycles("issue-1467-jsonpath-match-false", &[], vec![cycle])
        .remove(0);

    // Assert
    fixture::assert_success(&frames);
    assert_eq!(wire::data_rows(&frames), vec![vec![Some("f".to_owned())]]);
}

#[test]
fn should_propagate_null_jsonb_input_through_match_operator() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT NULL::JSONB @@ '$.a'::JSONPATH",
        None,
        0,
        true,
    );

    // Act
    let frames = fixture::run_cycles("issue-1467-jsonpath-operator-null-input", &[], vec![cycle])
        .remove(0);

    // Assert
    fixture::assert_success(&frames);
    assert_eq!(wire::data_rows(&frames), vec![vec![None]]);
}

#[test]
fn should_propagate_null_jsonb_input_through_match_function() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_path_match(NULL::JSONB, '$.a'::JSONPATH)",
        None,
        0,
        true,
    );

    // Act
    let frames = fixture::run_cycles("issue-1467-jsonpath-match-null-input", &[], vec![cycle])
        .remove(0);

    // Assert
    fixture::assert_success(&frames);
    assert_eq!(wire::data_rows(&frames), vec![vec![None]]);
}

#[test]
fn should_preserve_jsonpath_question_mark_exists_semantics() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT '{\"a\":1}'::JSONB @? '$.a'::JSONPATH",
        None,
        0,
        true,
    );

    // Act
    let frames = fixture::run_cycles("issue-1467-jsonpath-question-exists", &[], vec![cycle])
        .remove(0);

    // Assert
    fixture::assert_success(&frames);
    assert_eq!(wire::data_rows(&frames), vec![vec![Some("t".to_owned())]]);
}

#[test]
fn should_preserve_jsonpath_exists_function_semantics() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_path_exists('{\"a\":1}'::JSONB, '$.a'::JSONPATH)",
        None,
        0,
        true,
    );

    // Act
    let frames = fixture::run_cycles("issue-1467-jsonpath-exists-function", &[], vec![cycle])
        .remove(0);

    // Assert
    fixture::assert_success(&frames);
    assert_eq!(wire::data_rows(&frames), vec![vec![Some("t".to_owned())]]);
}

#[test]
fn should_return_null_for_jsonpath_match_on_json_null() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_path_match('null'::JSONB, '$'::JSONPATH)",
        None,
        0,
        true,
    );

    // Act
    let frames = fixture::run_cycles("issue-1467-jsonpath-match-json-null", &[], vec![cycle])
        .remove(0);

    // Assert
    fixture::assert_success(&frames);
    assert_eq!(wire::data_rows(&frames), vec![vec![None]]);
}

#[test]
fn should_return_null_for_timezone_aware_jsonpath_match_on_json_null() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_path_match_tz('null'::JSONB, '$'::JSONPATH)",
        None,
        0,
        true,
    );

    // Act
    let frames = fixture::run_cycles("issue-1467-jsonpath-match-tz-json-null", &[], vec![cycle])
        .remove(0);

    // Assert
    fixture::assert_success(&frames);
    assert_eq!(wire::data_rows(&frames), vec![vec![None]]);
}

#[test]
fn should_return_null_for_jsonpath_match_unknown_predicate() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_path_match('{\"a\":\"x\"}'::JSONB, '$.a > 1'::JSONPATH)",
        None,
        0,
        true,
    );

    // Act
    let frames = fixture::run_cycles("issue-1467-jsonpath-match-unknown-predicate", &[], vec![cycle])
        .remove(0);

    // Assert
    fixture::assert_success(&frames);
    assert_eq!(wire::data_rows(&frames), vec![vec![None]]);
}

#[test]
fn should_return_null_for_timezone_aware_match_unknown_predicate() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_path_match_tz('{\"a\":\"x\"}'::JSONB, '$.a > 1'::JSONPATH)",
        None,
        0,
        true,
    );

    // Act
    let frames = fixture::run_cycles("issue-1467-jsonpath-match-tz-unknown-predicate", &[], vec![cycle])
        .remove(0);

    // Assert
    fixture::assert_success(&frames);
    assert_eq!(wire::data_rows(&frames), vec![vec![None]]);
}

#[test]
fn should_reject_non_boolean_timezone_aware_jsonpath_match_results() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_path_match_tz('{\"a\":1}'::JSONB, '$.a'::JSONPATH)",
        None,
        0,
        true,
    );

    // Act
    let frames = fixture::run_cycles("issue-1467-jsonpath-match-tz", &[], vec![cycle]).remove(0);

    // Assert
    assert_eq!(wire::error_code(&frames).as_deref(), Some("22038"));
    let message = frames
        .iter()
        .find(|(tag, _)| *tag == b'E')
        .and_then(|(_, payload)| {
            wire::parse_error_fields(payload)
                .into_iter()
                .find(|(kind, _)| *kind == 'M')
                .map(|(_, message)| message)
        });
    assert_eq!(
        message.as_deref(),
        Some("single boolean result is expected")
    );
}

#[test]
fn should_return_null_when_timezone_aware_match_silently_suppresses_errors() {
    // Arrange
    let cases = [
        "SELECT jsonb_path_match_tz('{\"a\":1}'::JSONB, '$.a'::JSONPATH, '{}'::JSONB, true)",
        "SELECT jsonb_path_match_tz('{\"a\":1}'::JSONB, '$.missing'::JSONPATH, '{}'::JSONB, true)",
        "SELECT jsonb_path_match_tz('[true,false]'::JSONB, '$[*]'::JSONPATH, '{}'::JSONB, true)",
        "SELECT jsonb_path_match_tz('{\"a\":1}'::JSONB, 'strict $.missing'::JSONPATH, '{}'::JSONB, true)",
    ];
    let cycles = cases
        .iter()
        .map(|sql| fixture::execute_cycle(sql, None, 0, true))
        .collect();

    // Act
    let batches = fixture::run_cycles("issue-1467-jsonpath-match-tz-silent", &[], cycles);

    // Assert
    assert_eq!(batches.len(), cases.len());
    for (sql, frames) in cases.iter().zip(&batches) {
        fixture::assert_success(frames);
        assert_eq!(wire::data_rows(frames), vec![vec![None]], "{sql}");
    }
}

fn run_jsonpath_match(test_name: &str, document: &str, path: &str) -> fixture::Frames {
    let sql = format!("SELECT jsonb_path_match({document}, '{path}'::JSONPATH)");
    let cycle = fixture::execute_cycle(&sql, None, 0, true);
    let batches = fixture::run_cycles(test_name, &[], vec![cycle]);
    assert_eq!(batches.len(), 1, "one result cycle");
    batches.into_iter().next().expect("JSONPath result cycle")
}

fn assert_jsonpath_match_true(frames: &fixture::Frames) {
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONPath match RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size))
            .collect::<Vec<_>>(),
        vec![(16, 1)]
    );
    assert_eq!(wire::data_rows(frames), vec![vec![Some("t".to_owned())]]);
}
