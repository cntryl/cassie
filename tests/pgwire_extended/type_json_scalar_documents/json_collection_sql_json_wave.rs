//! JSON object expansion and SQL/JSON predicates/variables over pgwire.

#[path = "json_collection_sql_json_wave/function_alias_residuals.rs"]
mod function_alias_residuals;
#[path = "json_legacy_builders_and_paths.rs"]
mod json_legacy_builders_and_paths;

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

fn column_type_oids(frames: &fixture::Frames) -> Vec<i32> {
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("RowDescription");
    wire::parse_row_description(&description.1)
        .iter()
        .map(|column| column.type_oid)
        .collect()
}

#[test]
fn should_expand_json_each_as_json_typed_object_rows() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT key, value FROM json_each('{\"a\":null,\"b\":{\"k\":[1,2]}}') ORDER BY key",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-json-each-values", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(column_type_oids(frames), vec![25, 114]);
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![Some("a".to_owned()), Some("null".to_owned())],
            vec![Some("b".to_owned()), Some("{\"k\":[1,2]}".to_owned())],
        ]
    );
}

#[test]
fn should_expand_json_each_text_as_nullable_text_rows() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT key, value FROM json_each_text('{\"a\":null,\"b\":{\"k\":[1,2]}}') ORDER BY key",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-json-each-text-values", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(column_type_oids(frames), vec![25, 25]);
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![Some("a".to_owned()), None],
            vec![Some("b".to_owned()), Some("{\"k\":[1,2]}".to_owned())],
        ]
    );
}

#[test]
fn should_expand_jsonb_each_as_jsonb_typed_object_rows() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT key, value FROM jsonb_each('{\"a\":null,\"b\":{\"k\":[1,2]}}'::JSONB) ORDER BY key",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-829-jsonb-each-values", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(column_type_oids(frames), vec![25, 3_802]);
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![Some("a".to_owned()), Some("null".to_owned())],
            vec![Some("b".to_owned()), Some("{\"k\": [1, 2]}".to_owned())],
        ]
    );
}

#[test]
fn should_expand_jsonb_each_text_as_nullable_text_rows() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT key, value FROM jsonb_each_text('{\"a\":null,\"b\":{\"k\":[1,2]}}'::JSONB) ORDER BY key",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-829-jsonb-each-text-values", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(column_type_oids(frames), vec![25, 25]);
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![Some("a".to_owned()), None],
            vec![Some("b".to_owned()), Some("{\"k\": [1, 2]}".to_owned())],
        ]
    );
}

#[test]
fn should_classify_json_values_by_shape_with_sql_null_input() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT 'null' IS JSON VALUE, 'null' IS JSON SCALAR, '[]' IS JSON ARRAY, '{}' IS JSON OBJECT, '1' IS JSON OBJECT, '\"s\"' IS JSON SCALAR, NULL::TEXT IS JSON VALUE",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-sql-json-shape-predicates", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(column_type_oids(frames), vec![16; 7]);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            None,
        ]]
    );
}

#[test]
fn should_bind_passing_variables_in_sql_json_query_functions() {
    // Arrange
    let sql = "SELECT \
        JSON_VALUE('{\"n\":40}'::JSONB, '$.n + $increment' PASSING 2 AS increment RETURNING BIGINT), \
        JSON_QUERY('{\"items\":[{\"n\":1},{\"n\":2}]}'::JSONB, '$.items[*] ? (@.n == $desired)' PASSING 2 AS desired RETURNING JSONB), \
        JSON_EXISTS('{\"items\":[{\"n\":1},{\"n\":2}]}'::JSONB, '$.items[*] ? (@.n > $threshold)' PASSING 1 AS threshold)";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-810-sql-json-passing-variables", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(column_type_oids(frames), vec![20, 3_802, 16]);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("42".to_owned()),
            Some("{\"n\": 2}".to_owned()),
            Some("t".to_owned()),
        ]]
    );
}

#[test]
fn should_build_text_array_json_objects_with_jsonb_identity() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT \
            json_object(ARRAY['a','1','b','two']::TEXT[]), \
            jsonb_object(ARRAY['a','1','b','two']::TEXT[]), \
            json_object(ARRAY['a','b']::TEXT[], ARRAY['1','two']::TEXT[]), \
            jsonb_object(ARRAY['a','b']::TEXT[], ARRAY['1','two']::TEXT[])",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-legacy-text-array-json-objects", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(column_type_oids(frames), vec![114, 3_802, 114, 3_802]);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some(r#"{"a" : "1", "b" : "two"}"#.to_owned()),
            Some(r#"{"a": "1", "b": "two"}"#.to_owned()),
            Some(r#"{"a" : "1", "b" : "two"}"#.to_owned()),
            Some(r#"{"a": "1", "b": "two"}"#.to_owned()),
        ]]
    );
}
