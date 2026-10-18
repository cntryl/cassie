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
