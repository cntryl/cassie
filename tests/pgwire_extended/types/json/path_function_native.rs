use super::{fixture, wire};

#[test]
fn should_infer_finite_json_path_inputs_given_unknown_parameters_when_extracting_documents() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_extract_path_text('{\"a\":[1,null]}'::JSONB, $1, '-1')",
        Some((0, 0, Some(b"a"))),
        0,
        true,
    );
    // Act
    let batches = fixture::run_cycles("finite-json-path-parameter", &[], vec![cycle]);
    // Assert
    fixture::assert_success(&batches[0]);
    let parameters = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("parameters");
    assert_eq!(wire::parse_parameter_description(&parameters.1), [25]);
    assert_eq!(wire::data_rows(&batches[0]), vec![vec![None]]);
}

#[test]
fn should_preserve_finite_path_document_identity_given_document_nulls_when_extracting_values() {
    // Arrange
    let sql = "SELECT json_extract_path('{\"a\":null}'::JSON,'a'), jsonb_extract_path('{\"a\":null}'::JSONB,'a'), json_extract_path_text('{\"a\":null}'::JSON,'a'), jsonb_extract_path_text('{\"n\":123456789012345678901234567890.125}'::JSONB,'n'), jsonb_extract_path('{}'::JSONB,NULL)";
    // Act
    let batches = fixture::run_cycles(
        "finite-json-path-root",
        &[],
        vec![fixture::execute_cycle(sql, None, 0, true)],
    );
    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("null".into()),
            Some("null".into()),
            None,
            Some("123456789012345678901234567890.125".into()),
            None
        ]]
    );
}

#[test]
fn should_reject_finite_path_wrong_types_given_explicit_selectors_when_connections_are_reused() {
    // Arrange
    let cycles = ["SELECT jsonb_extract_path('{}'::JSONB, 1)", "SELECT 7::INT"]
        .map(|sql| fixture::execute_cycle(sql, None, 0, true));
    // Act
    let batches = fixture::run_cycles("finite-json-path-invalid", &[], cycles.into());
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42883"));
    fixture::assert_success(&batches[1]);
    assert_eq!(wire::data_rows(&batches[1]), vec![vec![Some("7".into())]]);
}

#[test]
fn should_infer_finite_path_document_parameters_given_unknown_types_when_describing_statements() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT jsonb_extract_path($1, 'a')",
        Some((0, 0, Some(b"{\"a\":null}"))),
        0,
        true,
    );
    // Act
    let batches = fixture::run_cycles("finite-json-path-document-parameter", &[], vec![cycle]);
    // Assert
    fixture::assert_success(&batches[0]);
    let parameters = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("parameters");
    assert_eq!(wire::parse_parameter_description(&parameters.1), [3802]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("null".into())]]
    );
}

#[test]
fn should_reject_nonpostgres_path_index_spacing_given_text_components_when_extracting_arrays() {
    // Arrange
    let sql = "SELECT json_extract_path('[1]'::JSON,'\u{a0}0'), jsonb_extract_path_text('[1]'::JSONB,'0 '), '[1]'::JSONB #> ARRAY['\u{a0}0'], jsonb_extract_path('[1]'::JSONB,' 0'), jsonb_extract_path('[1]'::JSONB,'+0')";
    // Act
    let batches = fixture::run_cycles(
        "finite-json-path-index-spacing",
        &[],
        vec![fixture::execute_cycle(sql, None, 0, true)],
    );
    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![None, None, None, Some("1".into()), Some("1".into())]]
    );
}
