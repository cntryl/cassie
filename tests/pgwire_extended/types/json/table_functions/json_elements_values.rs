//! Result values, NULL behaviour and descriptors for `json[b]_array_elements[_text]` and
//! `json[b]_each[_text]` used as FROM sources (#829 stage 1).

use super::json_elements_support::{column_names, column_oids, query, row, text_rows};
use super::{fixture, wire};

const MIXED_ELEMENTS: &str = r#"[1, "a", null, {"x":  1}, [2 , 3], 1.50]"#;

#[test]
fn should_expand_jsonb_array_elements_as_jsonb_rows_given_mixed_elements() {
    // Arrange
    let sql = format!("SELECT value FROM jsonb_array_elements('{MIXED_ELEMENTS}'::jsonb)");
    // Act
    let frames = query("json-elements-jsonb-array-values", &[], &sql, true);
    // Assert
    fixture::assert_success(&frames);
    assert_eq!(column_oids(&frames), vec![3802]);
    assert_eq!(
        wire::data_rows(&frames),
        text_rows(&["1", "\"a\"", "null", "{\"x\": 1}", "[2, 3]", "1.50"])
    );
}

#[test]
fn should_preserve_json_text_given_json_array_elements_when_expanding_json_input() {
    // Arrange
    let sql = format!("SELECT value FROM json_array_elements('{MIXED_ELEMENTS}'::json)");
    // Act
    let frames = query("json-elements-json-array-values", &[], &sql, true);
    // Assert
    fixture::assert_success(&frames);
    assert_eq!(column_oids(&frames), vec![114]);
    assert_eq!(
        wire::data_rows(&frames),
        text_rows(&["1", "\"a\"", "null", "{\"x\":  1}", "[2 , 3]", "1.50"])
    );
}

#[test]
fn should_return_text_given_jsonb_array_elements_text_with_json_null_as_sql_null() {
    // Arrange
    let sql = format!("SELECT value FROM jsonb_array_elements_text('{MIXED_ELEMENTS}'::jsonb)");
    // Act
    let frames = query("json-elements-jsonb-array-text", &[], &sql, true);
    // Assert
    fixture::assert_success(&frames);
    assert_eq!(column_oids(&frames), vec![25]);
    assert_eq!(
        wire::data_rows(&frames),
        vec![
            row(&[Some("1")]),
            row(&[Some("a")]),
            row(&[None]),
            row(&[Some("{\"x\": 1}")]),
            row(&[Some("[2, 3]")]),
            row(&[Some("1.50")]),
        ]
    );
}

#[test]
fn should_unescape_strings_given_json_array_elements_text_when_preserving_other_tokens() {
    // Arrange
    let sql =
        r#"SELECT value FROM json_array_elements_text('[1, "a\tb", null, {"x":  1}, true]'::json)"#;
    // Act
    let frames = query("json-elements-json-array-text", &[], sql, true);
    // Assert
    fixture::assert_success(&frames);
    assert_eq!(column_oids(&frames), vec![25]);
    assert_eq!(
        wire::data_rows(&frames),
        vec![
            row(&[Some("1")]),
            row(&[Some("a\tb")]),
            row(&[None]),
            row(&[Some("{\"x\":  1}")]),
            row(&[Some("true")]),
        ]
    );
}

#[test]
fn should_order_keys_canonically_given_jsonb_each_when_object_has_duplicate_keys() {
    // Arrange
    let sql = r#"SELECT key, value FROM jsonb_each('{"b":1, "a": "x", "b": null, "cc":{"z" : 1}, "aaa":[1,2]}'::jsonb)"#;
    // Act
    let frames = query("json-elements-jsonb-each", &[], sql, true);
    // Assert
    fixture::assert_success(&frames);
    assert_eq!(column_oids(&frames), vec![25, 3802]);
    assert_eq!(
        wire::data_rows(&frames),
        vec![
            row(&[Some("a"), Some("\"x\"")]),
            row(&[Some("b"), Some("null")]),
            row(&[Some("cc"), Some("{\"z\": 1}")]),
            row(&[Some("aaa"), Some("[1, 2]")]),
        ]
    );
}

#[test]
fn should_preserve_order_and_duplicates_given_json_each_when_expanding_json_input() {
    // Arrange
    let sql = r#"SELECT key, value FROM json_each('{"b":1, "a": "x", "b": null,  "cc":{"z" : 1}}'::json)"#;
    // Act
    let frames = query("json-elements-json-each", &[], sql, true);
    // Assert
    fixture::assert_success(&frames);
    assert_eq!(column_oids(&frames), vec![25, 114]);
    assert_eq!(
        wire::data_rows(&frames),
        vec![
            row(&[Some("b"), Some("1")]),
            row(&[Some("a"), Some("\"x\"")]),
            row(&[Some("b"), Some("null")]),
            row(&[Some("cc"), Some("{\"z\" : 1}")]),
        ]
    );
}

#[test]
fn should_return_text_values_given_each_text_functions_with_json_null_as_sql_null() {
    // Arrange
    let binary_sql = r#"SELECT key, value FROM jsonb_each_text('{"b":1, "a":"x", "c":null, "d":{"q":[1]}}'::jsonb)"#;
    let text_sql = r#"SELECT key, value FROM json_each_text('{"b":1, "a":"x", "b":null, "d":{"q" : [1]}}'::json)"#;
    // Act
    let binary_frames = query("json-elements-jsonb-each-text", &[], binary_sql, true);
    let text_frames = query("json-elements-json-each-text", &[], text_sql, true);
    // Assert
    fixture::assert_success(&binary_frames);
    fixture::assert_success(&text_frames);
    assert_eq!(column_oids(&binary_frames), vec![25, 25]);
    assert_eq!(column_oids(&text_frames), vec![25, 25]);
    assert_eq!(
        wire::data_rows(&binary_frames),
        vec![
            row(&[Some("a"), Some("x")]),
            row(&[Some("b"), Some("1")]),
            row(&[Some("c"), None]),
            row(&[Some("d"), Some("{\"q\": [1]}")]),
        ]
    );
    assert_eq!(
        wire::data_rows(&text_frames),
        vec![
            row(&[Some("b"), Some("1")]),
            row(&[Some("a"), Some("x")]),
            row(&[Some("b"), None]),
            row(&[Some("d"), Some("{\"q\" : [1]}")]),
        ]
    );
}

#[test]
fn should_name_columns_after_postgresql_given_unaliased_sources() {
    // Arrange
    let array = "SELECT * FROM jsonb_array_elements('[1]'::jsonb)";
    let each = "SELECT * FROM json_each('{\"a\":1}'::json)";
    // Act
    let array_frames = query("json-elements-array-names", &[], array, true);
    let each_frames = query("json-elements-each-names", &[], each, true);
    // Assert
    assert_eq!(column_names(&array_frames), vec!["value".to_owned()]);
    assert_eq!(
        column_names(&each_frames),
        vec!["key".to_owned(), "value".to_owned()]
    );
}

#[test]
fn should_return_no_rows_given_sql_null_input_for_every_selected_function() {
    // Arrange
    let calls = [
        "jsonb_array_elements(NULL::jsonb)",
        "jsonb_array_elements_text(NULL::jsonb)",
        "jsonb_each(NULL::jsonb)",
        "jsonb_each_text(NULL::jsonb)",
        "json_array_elements(NULL::json)",
        "json_array_elements_text(NULL::json)",
        "json_each(NULL::json)",
        "json_each_text(NULL::json)",
    ];
    for call in calls {
        // Act
        let frames = query(
            "json-elements-null-input",
            &[],
            &format!("SELECT * FROM {call}"),
            true,
        );
        // Assert
        fixture::assert_success(&frames);
        assert_eq!(wire::data_rows(&frames), Vec::<Vec<Option<String>>>::new());
    }
}

#[test]
fn should_return_no_rows_given_empty_containers_and_one_row_for_json_null_element() {
    // Arrange
    let empty_array = "SELECT * FROM jsonb_array_elements('[]'::jsonb)";
    let empty_object = "SELECT * FROM json_each('{}'::json)";
    let json_null = "SELECT value FROM jsonb_array_elements('[null]'::jsonb)";
    // Act
    let array = query("json-elements-empty-array", &[], empty_array, false);
    let object = query("json-elements-empty-object", &[], empty_object, false);
    let null = query("json-elements-null-element", &[], json_null, false);
    // Assert
    assert_eq!(wire::data_rows(&array), Vec::<Vec<Option<String>>>::new());
    assert_eq!(wire::data_rows(&object), Vec::<Vec<Option<String>>>::new());
    assert_eq!(wire::data_rows(&null), text_rows(&["null"]));
}

#[test]
fn should_apply_limit_given_array_elements_source_when_more_elements_exist() {
    // Arrange
    let sql = "SELECT value FROM jsonb_array_elements('[10,20,30,40]'::jsonb) LIMIT 2";
    // Act
    let frames = query("json-elements-limit", &[], sql, false);
    // Assert
    fixture::assert_success(&frames);
    assert_eq!(wire::data_rows(&frames), text_rows(&["10", "20"]));
}

#[test]
fn should_normalize_numbers_and_order_nested_keys_given_jsonb_text_expansion() {
    // Arrange
    let sql = r#"SELECT value FROM jsonb_array_elements_text('[1e2, {"bb":1,"a":2,"ccc":{"zz":1,"y":2}}]'::jsonb)"#;
    // Act
    let frames = query("json-elements-jsonb-text-normalized", &[], sql, false);
    // Assert
    fixture::assert_success(&frames);
    assert_eq!(
        wire::data_rows(&frames),
        text_rows(&[
            "100",
            "{\"a\": 2, \"bb\": 1, \"ccc\": {\"y\": 2, \"zz\": 1}}"
        ])
    );
}
