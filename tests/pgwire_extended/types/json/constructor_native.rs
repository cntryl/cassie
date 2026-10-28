use super::{fixture, wire};

#[test]
fn should_preserve_json_constructor_identity_given_documents_and_duplicate_keys_when_building_objects(
) {
    // Arrange
    let cycle = fixture::execute_cycle("SELECT jsonb_build_object('doc','{\"k\":1}'::JSON,'text','{\"k\":1}'::TEXT), json_build_object('a',1,'a',2), jsonb_build_object('a',1,'a',2)", None, 0, true);
    // Act
    let batches = fixture::run_cycles("json-native-builder-documents", &[], vec![cycle]);
    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some(r#"{"doc": {"k": 1}, "text": "{\"k\":1}"}"#.into()),
            Some(r#"{"a" : 1, "a" : 2}"#.into()),
            Some(r#"{"a": 2}"#.into())
        ]]
    );
    let description = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("builder descriptors");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [3802, 114, 3802]
    );
}

#[test]
fn should_preserve_json_conversion_values_given_exact_numbers_and_nulls_when_building_documents() {
    // Arrange
    let cycle = fixture::execute_cycle("SELECT jsonb_build_array(123456789012345678901234567890.125::NUMERIC, 'NaN'::FLOAT8), to_jsonb(NULL::INT), to_jsonb('null'::JSONB)", None, 0, true);
    // Act
    let batches = fixture::run_cycles("json-native-builder-exact-null", &[], vec![cycle]);
    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some(r#"[123456789012345678901234567890.125, "NaN"]"#.into()),
            None,
            Some("null".into())
        ]]
    );
}

#[test]
fn should_reject_json_object_constructor_keys_given_invalid_inputs_when_the_connection_is_reused() {
    // Arrange
    let cases = [
        ("SELECT jsonb_build_object('a')", "22023"),
        ("SELECT jsonb_build_object(NULL, 1)", "22023"),
        ("SELECT jsonb_build_object('{}'::JSONB, 1)", "22023"),
        ("SELECT to_json('unknown')", "42804"),
    ];
    let cycles = cases
        .iter()
        .flat_map(|(sql, _)| {
            [
                fixture::execute_cycle(sql, None, 0, true),
                fixture::execute_cycle("SELECT 7::INT", None, 0, true),
            ]
        })
        .collect();
    // Act
    let batches = fixture::run_cycles("json-native-builder-invalid-keys", &[], cycles);
    // Assert
    for ((_, code), frames) in cases.iter().zip(batches.as_chunks::<2>().0) {
        assert_eq!(wire::error_code(&frames[0]).as_deref(), Some(*code));
        assert_eq!(
            wire::data_rows(&frames[0]),
            Vec::<Vec<Option<String>>>::new()
        );
        fixture::assert_success(&frames[1]);
        assert_eq!(wire::data_rows(&frames[1]), vec![vec![Some("7".into())]]);
    }
}

#[test]
fn should_infer_empty_jsonb_constructor_comparands_given_unknown_parameters_when_describing_statements(
) {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_build_object() = $1",
        Some((0, 0, Some(b"{}"))),
        0,
        true,
    );
    // Act
    let batches = fixture::run_cycles("jsonb-empty-constructor-comparand", &[], vec![cycle]);
    // Assert
    fixture::assert_success(&batches[0]);
    let parameters = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("parameter description");
    assert_eq!(wire::parse_parameter_description(&parameters.1), [3802]);
    assert_eq!(wire::data_rows(&batches[0]), vec![vec![Some("t".into())]]);
}

#[test]
fn should_preserve_json_conversion_output_types_given_polymorphic_inputs_when_calls_are_nested() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_typeof(to_jsonb(42)), to_jsonb(42) = $1",
        Some((0, 0, Some(b"42"))),
        0,
        true,
    );
    // Act
    let batches = fixture::run_cycles("jsonb-conversion-nested-output-type", &[], vec![cycle]);
    // Assert
    fixture::assert_success(&batches[0]);
    let parameters = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("parameter description");
    assert_eq!(wire::parse_parameter_description(&parameters.1), [3802]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("number".into()), Some("t".into())]]
    );
}
