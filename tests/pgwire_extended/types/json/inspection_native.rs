use super::{fixture, wire};

#[test]
fn should_infer_native_jsonb_inspection_parameters_given_unknown_input_when_describing_statements()
{
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_typeof($1)",
        Some((0, 0, Some(b"{\"a\":1}"))),
        0,
        true,
    );
    // Act
    let batches = fixture::run_cycles("jsonb-inspection-parameter-type", &[], vec![cycle]);
    // Assert
    fixture::assert_success(&batches[0]);
    let parameters = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("parameter description");
    assert_eq!(wire::parse_parameter_description(&parameters.1), [3802]);
    fixture::assert_single_column_descriptors(&batches[0], (25, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("object".into())]]
    );
}

#[test]
fn should_reject_json_inspection_signatures_given_incompatible_types_when_binding_functions() {
    // Arrange
    let cycles = [
        "SELECT jsonb_typeof(1)",
        "SELECT json_typeof('{}'::JSONB)",
        "SELECT jsonb_exists_any('{}'::JSONB, '{a}'::TEXT)",
        "SELECT jsonb_array_length()",
    ]
    .map(|sql| fixture::execute_cycle(sql, None, 0, true))
    .to_vec();
    // Act
    let batches = fixture::run_cycles("json-inspection-invalid-signatures", &[], cycles);
    // Assert
    for frames in batches {
        assert_eq!(wire::error_code(&frames).as_deref(), Some("42883"));
        assert_eq!(wire::data_rows(&frames), Vec::<Vec<Option<String>>>::new());
    }
}

#[test]
fn should_encode_jsonb_array_lengths_given_binary_results_when_returning_int4() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_array_length('[1,null,3]'::JSONB)",
        None,
        1,
        true,
    );
    // Act
    let batches = fixture::run_cycles("jsonb-array-length-binary", &[], vec![cycle]);
    // Assert
    fixture::assert_success(&batches[0]);
    fixture::assert_single_column_descriptors(&batches[0], (23, 4, -1), 1, true);
    assert_eq!(
        fixture::data_row_payloads(&batches[0]),
        vec![fixture::one_field_row(&3_i32.to_be_bytes())]
    );
}

#[test]
fn should_match_jsonb_string_scalars_given_key_inspection_when_existence_is_evaluated() {
    // Arrange
    let cycle = fixture::execute_cycle("SELECT '\"foo\"'::JSONB ? 'foo', '\"foo\"'::JSONB ?| '{foo}', '\"foo\"'::JSONB ?& '{foo}', jsonb_exists_any('\"foo\"'::JSONB, '{foo}'), jsonb_exists_all('\"foo\"'::JSONB, '{foo}'), jsonb_exists_any('1'::JSONB, '{1}')", None, 0, true);
    // Act
    let batches = fixture::run_cycles("jsonb-scalar-string-existence", &[], vec![cycle]);
    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("t".into()),
            Some("t".into()),
            Some("t".into()),
            Some("t".into()),
            Some("t".into()),
            Some("f".into())
        ]]
    );
}

#[test]
fn should_preserve_declared_json_inspection_functions_given_name_collisions_when_qualified_builtins_are_available(
) {
    // Arrange
    let setup = [(
        "CREATE FUNCTION jsonb_typeof(x TEXT) RETURNS TEXT AS \"'custom'\"",
        Vec::new(),
    )];
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_typeof('hello'::TEXT), pg_catalog.jsonb_typeof('{}'::JSONB)",
        None,
        0,
        true,
    );
    // Act
    let batches = fixture::run_cycles(
        "json-inspection-declared-function-identity",
        &setup,
        vec![cycle],
    );
    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("custom".into()), Some("object".into())]]
    );
}

#[test]
fn should_accept_declared_document_parameters_given_native_oids_when_inspection_functions_are_bound(
) {
    // Arrange
    let cycles = vec![
        fixture::execute_cycle(
            "SELECT jsonb_typeof($1)",
            Some((3802, 1, Some(b"\x01{}"))),
            0,
            true,
        ),
        fixture::execute_cycle(
            "SELECT json_typeof($1)",
            Some((114, 1, Some(b"{}"))),
            0,
            true,
        ),
    ];
    // Act
    let batches = fixture::run_cycles("json-inspection-declared-parameter-oids", &[], cycles);
    // Assert
    for (frames, oid) in batches.iter().zip([3802, 114]) {
        fixture::assert_success(frames);
        let parameters = frames
            .iter()
            .find(|(tag, _)| *tag == b't')
            .expect("parameter description");
        assert_eq!(wire::parse_parameter_description(&parameters.1), [oid]);
        assert_eq!(wire::data_rows(frames), vec![vec![Some("object".into())]]);
    }
}
