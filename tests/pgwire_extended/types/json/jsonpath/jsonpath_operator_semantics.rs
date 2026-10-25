//! PostgreSQL jsonpath operators and timezone-aware function variants.

use super::super::support_pgwire as wire;
use super::super::support_type_metadata_contract as fixture;

#[test]
fn should_apply_recursive_jsonpath_accessors_with_exists_operator() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT '{\"root\":{\"items\":[{\"target\":\"hit\"}]}}'::JSONB @? '$.**.target ? (@ == \"hit\")'::JSONPATH",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonpath-recursive-accessor", &[], vec![cycle]);

    // Assert
    assert_boolean_rows(&batches[0], Some("t"));
}

#[test]
fn should_select_last_array_item_with_match_operator() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT '[10,20,30]'::JSONB @@ '$[last] == 30'::JSONPATH",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonpath-last-accessor", &[], vec![cycle]);

    // Assert
    assert_boolean_rows(&batches[0], Some("t"));
}

#[test]
fn should_match_jsonpath_regular_expressions_case_insensitively() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT '{\"names\":[\"Ada\",\"Linus\"]}'::JSONB @? '$.names[*] ? (@ like_regex \"^a\" flag \"i\")'::JSONPATH",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonpath-like-regex", &[], vec![cycle]);

    // Assert
    assert_boolean_rows(&batches[0], Some("t"));
}

#[test]
fn should_match_jsonpath_string_prefixes() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT '{\"names\":[\"Ada\",\"Linus\"]}'::JSONB @? '$.names[*] ? (@ starts with \"Ada\")'::JSONPATH",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonpath-starts-with", &[], vec![cycle]);

    // Assert
    assert_boolean_rows(&batches[0], Some("t"));
}

#[test]
fn should_test_unknown_jsonpath_predicates_after_strict_errors() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT '{\"x\":{}}'::JSONB @? 'strict $.x ? ((exists(@.name)) is unknown)'::JSONPATH",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonpath-is-unknown", &[], vec![cycle]);

    // Assert
    assert_boolean_rows(&batches[0], Some("t"));
}

#[test]
fn should_search_jsonpath_keyvalue_results() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT '{\"foo\":1,\"bar\":2}'::JSONB @? '$.keyvalue() ? (@.key == \"foo\" && @.value == 1)'::JSONPATH",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonpath-keyvalue", &[], vec![cycle]);

    // Assert
    assert_boolean_rows(&batches[0], Some("t"));
}

#[test]
fn should_apply_jsonpath_scalar_conversion_methods_with_match_operator() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT \
            '{\"enabled\":true}'::JSONB @@ '$.enabled.boolean() == true'::JSONPATH, \
            '{\"name\":\"Ada\"}'::JSONB @@ '$.name.string() == \"Ada\"'::JSONPATH, \
            '{\"fraction\":12.5}'::JSONB @@ '$.fraction.double() == 12.5'::JSONPATH, \
            '{\"whole\":42}'::JSONB @@ '$.whole.bigint() == 42'::JSONPATH, \
            '{\"fraction\":12.5}'::JSONB @@ '$.fraction.decimal(5,2) == 12.50'::JSONPATH, \
            '{\"whole\":42}'::JSONB @@ '$.whole.integer() == 42'::JSONPATH, \
            '{\"fraction\":12.5}'::JSONB @@ '$.fraction.number() == 12.5'::JSONPATH",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "issue-809-jsonpath-scalar-conversion-methods",
        &[],
        vec![cycle],
    );

    // Assert
    assert_boolean_columns(&batches[0], 7);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("t".to_owned()); 7]]
    );
}

#[test]
fn should_apply_jsonpath_rounding_methods_with_match_operator() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT \
            '{\"fraction\":12.5}'::JSONB @@ '$.fraction.ceiling() == 13'::JSONPATH, \
            '{\"fraction\":12.5}'::JSONB @@ '$.fraction.floor() == 12'::JSONPATH",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonpath-rounding-methods", &[], vec![cycle]);

    // Assert
    assert_boolean_columns(&batches[0], 2);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("t".to_owned()); 2]]
    );
}

#[test]
fn should_compare_jsonpath_datetimes_through_timezone_aware_functions() {
    // Arrange
    let document = "'{\"ts\":\"2024-03-10T02:30:00-05:00\"}'::JSONB";
    let exists = fixture::execute_cycle(
        &format!(
            "SELECT jsonb_path_exists_tz({document}, '$.ts.datetime() ? (@ == \"2024-03-10T07:30:00Z\".datetime())'::JSONPATH)"
        ),
        None,
        0,
        true,
    );
    let matches = fixture::execute_cycle(
        &format!(
            "SELECT jsonb_path_match_tz({document}, '$.ts.datetime() == \"2024-03-10T07:30:00Z\".datetime()'::JSONPATH)"
        ),
        None,
        0,
        true,
    );
    let query = fixture::execute_cycle(
        &format!("SELECT jsonb_path_query_tz({document}, '$.ts.datetime()'::JSONPATH)"),
        None,
        0,
        true,
    );
    let query_array = fixture::execute_cycle(
        &format!("SELECT jsonb_path_query_array_tz({document}, '$.ts.datetime()'::JSONPATH)"),
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "issue-809-jsonpath-timezone-aware-datetime",
        &[],
        vec![exists, matches, query, query_array],
    );

    // Assert
    assert_eq!(batches.len(), 4);
    assert_boolean_rows(&batches[0], Some("t"));
    assert_boolean_rows(&batches[1], Some("t"));
    assert_jsonb_rows(&batches[2], Some("\"2024-03-10T02:30:00-05:00\""));
    assert_jsonb_rows(&batches[3], Some("[\"2024-03-10T02:30:00-05:00\"]"));
}

#[test]
fn should_return_the_first_jsonpath_item_or_sql_null_when_no_item_matches() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_path_query_first('{\"items\":[4,5]}'::JSONB, '$.items[*]'), jsonb_path_query_first('{\"items\":[4,5]}'::JSONB, '$.missing')",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonpath-query-first", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONB query_first RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![3802, 3802]
    );
    assert_eq!(wire::data_rows(frames), vec![vec![Some("4".into()), None]]);
}

fn assert_boolean_rows(frames: &fixture::Frames, expected: Option<&str>) {
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONPath boolean RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(16, 1, -1)]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![expected.map(str::to_owned)]]
    );
}

fn assert_boolean_columns(frames: &fixture::Frames, expected_count: usize) {
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONPath boolean RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(16, 1, -1); expected_count]
    );
}

fn assert_jsonb_rows(frames: &fixture::Frames, expected: Option<&str>) {
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONB RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(3802, -1, -1)]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![expected.map(str::to_owned)]]
    );
}
