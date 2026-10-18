//! JSON object expansion and SQL/JSON predicates/variables over pgwire.

#[path = "../function_alias/errors.rs"]
mod function_alias_residuals;
#[path = "../json_legacy_builders_and_paths.rs"]
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

include!("parameters.rs");

include!("semantics.rs");
