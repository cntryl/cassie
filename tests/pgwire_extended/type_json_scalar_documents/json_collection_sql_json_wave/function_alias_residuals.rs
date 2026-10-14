//! Uncovered JSON versus JSONB inspection and conversion contracts for #810.

use super::super::{fixture, wire};

fn column_signatures(frames: &fixture::Frames) -> Vec<(i32, i16, i32)> {
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("RowDescription");
    wire::parse_row_description(&description.1)
        .iter()
        .map(|column| (column.type_oid, column.type_size, column.type_mod))
        .collect()
}

#[test]
fn should_return_json_array_lengths_with_sql_nulls() {
    // Arrange
    let sql = "SELECT json_array_length('[1,null,3]'::JSON), \
        jsonb_array_length('[1,null,3]'::JSONB), \
        json_array_length('[]'::JSON), jsonb_array_length('[]'::JSONB), \
        json_array_length(NULL::JSON), jsonb_array_length(NULL::JSONB)";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-810-json-array-length-null-aliases", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(column_signatures(frames), vec![(23, 4, -1); 6]);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("3".to_owned()),
            Some("3".to_owned()),
            Some("0".to_owned()),
            Some("0".to_owned()),
            None,
            None,
        ]]
    );
}

#[test]
fn should_reject_nonarray_json_array_length_inputs_with_22023() {
    // Arrange
    let json = fixture::execute_cycle("SELECT json_array_length('{}'::JSON)", None, 0, true);
    let jsonb = fixture::execute_cycle("SELECT jsonb_array_length('{}'::JSONB)", None, 0, true);
    let recovery = fixture::execute_cycle("SELECT 42", None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-810-json-array-length-nonarray-error",
        &[],
        vec![json, jsonb, recovery],
    );

    // Assert
    for frames in &batches[..2] {
        assert_eq!(wire::error_code(frames).as_deref(), Some("22023"));
        assert!(wire::data_rows(frames).is_empty());
        assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'C').count(), 0);
        assert_eq!(frames.last().map(|frame| frame.0), Some(b'Z'));
    }
    fixture::assert_success(&batches[2]);
    assert_eq!(
        wire::data_rows(&batches[2]),
        vec![vec![Some("42".to_owned())]]
    );
}

#[test]
fn should_preserve_json_builder_result_type_identities() {
    // Arrange
    let sql = "SELECT json_build_object('wide',9007199254740993,'keep',NULL), \
        jsonb_build_object('wide',9007199254740993,'keep',NULL), \
        json_build_array(1,NULL,'x'), jsonb_build_array(1,NULL,'x')";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-810-json-builder-alias-oids", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        column_signatures(frames),
        vec![
            (114, -1, -1),
            (3_802, -1, -1),
            (114, -1, -1),
            (3_802, -1, -1)
        ]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some(r#"{"wide" : 9007199254740993, "keep" : null}"#.to_owned()),
            Some(r#"{"keep": null, "wide": 9007199254740993}"#.to_owned()),
            Some(r#"[1, null, "x"]"#.to_owned()),
            Some(r#"[1, null, "x"]"#.to_owned()),
        ]]
    );
}

#[test]
fn should_preserve_to_json_result_type_identity() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT to_json(42), to_json('Ada'::TEXT), to_jsonb(42)",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-810-to-json-type-identity", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        column_signatures(frames),
        vec![(114, -1, -1), (114, -1, -1), (3_802, -1, -1)]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("42".to_owned()),
            Some(r#""Ada""#.to_owned()),
            Some("42".to_owned()),
        ]]
    );
}

#[test]
fn should_expand_json_object_keys_as_text_rows() {
    // Arrange
    let sql = "SELECT json_object_keys('{\"z\":1,\"a\":2}'::JSON) ORDER BY 1";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-810-json-object-keys-json-alias", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(column_signatures(frames), vec![(25, -1, -1)]);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("a".to_owned())], vec![Some("z".to_owned())]]
    );
}

#[test]
fn should_strip_json_null_fields_under_the_array_option() {
    // Arrange
    let document = r#"{"drop":null,"items":[null,1]}"#;
    let sql = format!(
        "SELECT json_strip_nulls('{document}'::JSON), \
            jsonb_strip_nulls('{document}'::JSONB), \
            json_strip_nulls('{document}'::JSON, TRUE), \
            jsonb_strip_nulls('{document}'::JSONB, TRUE)"
    );
    let cycle = fixture::execute_cycle(&sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-810-json-strip-nulls-array-option", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        column_signatures(frames),
        vec![
            (114, -1, -1),
            (3_802, -1, -1),
            (114, -1, -1),
            (3_802, -1, -1)
        ]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some(r#"{"items":[null,1]}"#.to_owned()),
            Some(r#"{"items": [null, 1]}"#.to_owned()),
            Some(r#"{"items":[1]}"#.to_owned()),
            Some(r#"{"items": [1]}"#.to_owned()),
        ]]
    );
}

#[test]
fn should_apply_empty_or_null_key_arrays_to_jsonb_existence_functions() {
    // Arrange
    let sql = "SELECT jsonb_exists_any('{\"a\":1}'::JSONB, '{}'::TEXT[]), \
        jsonb_exists_all('{\"a\":1}'::JSONB, '{}'::TEXT[]), \
        jsonb_exists_any(NULL::JSONB, '{a}'::TEXT[]), \
        jsonb_exists_all('{\"a\":1}'::JSONB, NULL::TEXT[]), \
        jsonb_exists_any('null'::JSONB, '{a}'::TEXT[]), \
        jsonb_exists_all('null'::JSONB, '{}'::TEXT[])";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-810-jsonb-exists-null-empty-keys", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(column_signatures(frames), vec![(16, 1, -1); 6]);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("f".to_owned()),
            Some("t".to_owned()),
            None,
            None,
            Some("f".to_owned()),
            Some("t".to_owned()),
        ]]
    );
}

#[test]
fn should_apply_jsonb_set_lax_null_treatments() {
    // Arrange
    let sql = "SELECT \
        jsonb_set_lax('{\"a\":1,\"b\":2}'::JSONB, '{a}', NULL::JSONB, TRUE, 'use_json_null'), \
        jsonb_set_lax('{\"a\":1,\"b\":2}'::JSONB, '{a}', NULL::JSONB, TRUE, 'return_target'), \
        jsonb_set_lax('{\"a\":1,\"b\":2}'::JSONB, '{a}', NULL::JSONB, TRUE, 'delete_key'), \
        jsonb_set_lax('{\"a\":1,\"b\":2}'::JSONB, '{missing}', NULL::JSONB, TRUE, 'delete_key')";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-810-jsonb-set-lax-null-treatments", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(column_signatures(frames), vec![(3_802, -1, -1); 4]);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some(r#"{"a": null, "b": 2}"#.to_owned()),
            Some(r#"{"a": 1, "b": 2}"#.to_owned()),
            Some(r#"{"b": 2}"#.to_owned()),
            Some(r#"{"a": 1, "b": 2}"#.to_owned()),
        ]]
    );
}

#[test]
fn should_reject_jsonb_set_lax_null_raise_without_partial_rows() {
    // Arrange
    let malformed = fixture::execute_cycle(
        "SELECT jsonb_set_lax('{\"a\":1}'::JSONB, '{a}', NULL::JSONB, TRUE, 'raise_exception'), 42",
        None,
        0,
        true,
    );
    let recovery = fixture::execute_cycle("SELECT 42", None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-810-jsonb-set-lax-raise-exception",
        &[],
        vec![malformed, recovery],
    );

    // Assert
    let frames = &batches[0];
    assert_eq!(wire::error_code(frames).as_deref(), Some("22004"));
    assert!(wire::data_rows(frames).is_empty());
    assert_eq!(frames.iter().filter(|(tag, _)| *tag == b'C').count(), 0);
    fixture::assert_success(&batches[1]);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("42".to_owned())]]
    );
}
