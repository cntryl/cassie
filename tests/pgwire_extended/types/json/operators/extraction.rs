use super::{fixture, wire};

#[test]
fn should_resolve_binary_integer_parameters_as_jsonb_array_selectors() {
    // Arrange
    let index = 1_i32.to_be_bytes();
    let cycle = fixture::execute_cycle(
        "SELECT '[10,20]'::JSONB->$1",
        Some((23, 1, Some(&index))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("jsonb-extract-integer-parameter", &[], vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    let parameters = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("parameter description");
    assert_eq!(wire::parse_parameter_description(&parameters.1), [23]);
    assert_eq!(wire::data_rows(&batches[0]), vec![vec![Some("20".into())]]);
}

#[test]
fn should_reject_declared_invalid_jsonb_extraction_parameter_types() {
    // Arrange
    let cycles = vec![
        fixture::execute_cycle(
            "SELECT '[10,20]'::JSONB->$1",
            Some((20, 0, Some(b"1"))),
            0,
            true,
        ),
        fixture::execute_cycle(
            "SELECT '{}'::JSONB#>$1",
            Some((25, 0, Some(b"{a}"))),
            0,
            true,
        ),
        fixture::execute_cycle("SELECT $1->'a'", Some((23, 0, Some(b"1"))), 0, true),
    ];

    // Act
    let batches = fixture::run_cycles("jsonb-extract-parameter-overloads", &[], cycles);

    // Assert
    for frames in &batches {
        assert_eq!(wire::error_code(frames).as_deref(), Some("42883"));
        assert_eq!(wire::data_rows(frames), Vec::<Vec<Option<String>>>::new());
    }
}

#[test]
fn should_retain_json_document_identity_through_chained_scalar_extraction() {
    // Arrange
    let sql = "SELECT ('{\"a\":\"text\"}'::JSONB->'a')->>0, \
        (('{\"a\":null}'::JSONB->'a')->0) IS NULL, \
        ('{\"a\":1}'::JSONB->'a')->>-1, \
        (('{}'::JSONB->'missing')->0) IS NULL, \
        (('{\"a\":null}'::JSONB->'a')->>0) IS NULL";

    // Act
    let batches = fixture::run_cycles(
        "jsonb-extract-scalar-chain",
        &[],
        vec![fixture::execute_cycle(sql, None, 0, true)],
    );

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("text".into()),
            Some("f".into()),
            Some("1".into()),
            Some("t".into()),
            Some("t".into())
        ]]
    );
}

#[test]
fn should_extract_jsonb_array_elements_with_signed_integer_selectors() {
    // Arrange
    let sql = "SELECT '[10,20,30]'::JSONB->0, '[10,20,30]'::JSONB->-1, \
        '[10,20,30]'::JSONB->>-3, '[10,20,30]'::JSONB->3, \
        '[10,20,30]'::JSONB->-4, '[10,20,30]'::JSONB->'1', \
        '{\"1\":\"key\"}'::JSONB->>'1'";

    // Act
    let batches = fixture::run_cycles(
        "jsonb-extract-array",
        &[],
        vec![fixture::execute_cycle(sql, None, 0, true)],
    );

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("10".into()),
            Some("30".into()),
            Some("10".into()),
            None,
            None,
            None,
            Some("key".into())
        ]]
    );
}

#[test]
fn should_distinguish_jsonb_scalar_indexing_from_nested_path_traversal() {
    // Arrange
    let sql = "SELECT '1'::JSONB->0, '1'::JSONB->-1, '1'::JSONB->1, \
        'null'::JSONB->0, '\"foo\"'::JSONB->0, '1'::JSONB#>'{0}', \
        '{\"a\":1}'::JSONB#>'{a,0}', '[1]'::JSONB#>'{0,0}'";

    // Act
    let batches = fixture::run_cycles(
        "jsonb-extract-scalar",
        &[],
        vec![fixture::execute_cycle(sql, None, 0, true)],
    );

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("1".into()),
            Some("1".into()),
            None,
            Some("null".into()),
            Some("\"foo\"".into()),
            None,
            None,
            None
        ]]
    );
}

#[test]
fn should_preserve_jsonb_extraction_null_semantics() {
    // Arrange
    let sql = "SELECT '{\"a\":null}'::JSONB->'a', '{\"a\":null}'::JSONB->>'a', \
        '{}'::JSONB->'a', NULL::JSONB->'a', '{}'::JSONB->NULL::TEXT, \
        '{\"a\":null}'::JSONB#>'{a}', '{\"a\":null}'::JSONB#>>'{a}'";

    // Act
    let batches = fixture::run_cycles(
        "jsonb-extract-nulls",
        &[],
        vec![fixture::execute_cycle(sql, None, 0, true)],
    );

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("null".into()),
            None,
            None,
            None,
            None,
            Some("null".into()),
            None
        ]]
    );
}

#[test]
fn should_preserve_jsonb_extraction_operator_precedence() {
    // Arrange
    let sql = "SELECT '{\"items\":[{\"n\":3}]}'::JSONB->'items'->0->>'n', \
        '{\"n\":3}'::JSONB->>'n'='3', '[10,20,30]'::JSONB->1+1, \
        ('{\"a\":1}'::JSONB||'{\"b\":2}'::JSONB)->>'b'";

    // Act
    let batches = fixture::run_cycles(
        "jsonb-extract-precedence",
        &[],
        vec![fixture::execute_cycle(sql, None, 0, true)],
    );

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("3".into()),
            Some("t".into()),
            Some("30".into()),
            Some("2".into())
        ]]
    );
}

#[test]
fn should_preserve_exact_jsonb_extraction_values() {
    // Arrange
    let sql = "SELECT '{\"n\":0,\"n\":18446744073709551615}'::JSONB->'n', \
        '{\"n\":9007199254740993}'::JSONB->>'n', \
        '{\"a\":{\"n\":123456789012345678901234567890.125}}'::JSONB#>'{a,n}', \
        '{\"é\":\"雪\"}'::JSONB->>'é'";

    // Act
    let batches = fixture::run_cycles(
        "jsonb-extract-exact",
        &[],
        vec![fixture::execute_cycle(sql, None, 0, true)],
    );

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("18446744073709551615".into()),
            Some("9007199254740993".into()),
            Some("123456789012345678901234567890.125".into()),
            Some("雪".into())
        ]]
    );
}

#[test]
fn should_preserve_jsonb_path_extraction_boundary_semantics() {
    // Arrange
    let sql = "SELECT '{\"a\":1}'::JSONB#>'{}', 'null'::JSONB#>>'{}', \
        '\"text\"'::JSONB#>>'{}', '[1,2]'::JSONB#>'{NULL}', \
        '[1,2]'::JSONB#>'{not-an-index}', '[1,2]'::JSONB#>'{2147483648}', \
        '[1,2]'::JSONB#>'{-2147483649}', '[1,2]'::JSONB#>'{9}'";

    // Act
    let batches = fixture::run_cycles(
        "jsonb-extract-path-boundaries",
        &[],
        vec![fixture::execute_cycle(sql, None, 0, true)],
    );

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("{\"a\": 1}".into()),
            None,
            Some("text".into()),
            None,
            None,
            None,
            None,
            None
        ]]
    );
}

#[test]
fn should_resolve_jsonb_path_segments_by_container_type() {
    // Arrange
    let sql = "SELECT '{\"items\":[{\"1\":\"first\"},{\"1\":\"last\"}]}'::JSONB#>>'{items,-1,1}', \
        '[10,20]'::JSONB#>'{ +1 }', '[10,20]'::JSONB#>'{ 1}', \
        '{\"a,b\":true}'::JSONB#>>'{\"a,b\"}'";

    // Act
    let batches = fixture::run_cycles(
        "jsonb-extract-path-segments",
        &[],
        vec![fixture::execute_cycle(sql, None, 0, true)],
    );

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![
            Some("last".into()),
            Some("20".into()),
            Some("20".into()),
            Some("true".into())
        ]]
    );
}

#[test]
fn should_infer_text_parameters_for_jsonb_object_field_extraction() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT '{\"key\":42}'::JSONB->>$1",
        Some((0, 0, Some(b"key"))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("jsonb-extract-key-parameter", &[], vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    let parameters = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("parameter description");
    assert_eq!(wire::parse_parameter_description(&parameters.1), [25]);
    assert_eq!(wire::data_rows(&batches[0]), vec![vec![Some("42".into())]]);
}

#[test]
fn should_infer_text_array_parameters_for_jsonb_path_extraction() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT '{\"a\":[1,2]}'::JSONB#>>$1",
        Some((0, 0, Some(b"{a,-1}"))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("jsonb-extract-path-parameter", &[], vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    let parameters = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("parameter description");
    assert_eq!(wire::parse_parameter_description(&parameters.1), [1009]);
    assert_eq!(wire::data_rows(&batches[0]), vec![vec![Some("2".into())]]);
}

#[test]
fn should_preserve_jsonb_extraction_wire_descriptors() {
    // Arrange
    let cycles = [
        "SELECT '{\"a\":[1,2]}'::JSONB->'a'",
        "SELECT '{\"a\":\"text\"}'::JSONB->>'a'",
    ]
    .map(|sql| fixture::execute_cycle(sql, None, 1, true));

    // Act
    let batches = fixture::run_cycles("jsonb-extract-binary", &[], Vec::from(cycles));

    // Assert
    fixture::assert_success(&batches[0]);
    fixture::assert_success(&batches[1]);
    fixture::assert_single_column_descriptors(&batches[0], (3802, -1, -1), 1, true);
    fixture::assert_single_column_descriptors(&batches[1], (25, -1, -1), 1, true);
    assert_eq!(
        fixture::data_row_payloads(&batches[0]),
        vec![fixture::one_field_row(b"\x01[1, 2]")]
    );
    assert_eq!(
        fixture::data_row_payloads(&batches[1]),
        vec![fixture::one_field_row(b"text")]
    );
}

#[test]
fn should_reject_invalid_jsonb_extraction_signatures_before_rows_are_returned() {
    // Arrange
    let queries = [
        "SELECT 1->'a'",
        "SELECT '{}'::JSONB->TRUE",
        "SELECT '{}'::JSONB->1::BIGINT",
        "SELECT '{}'::JSONB#>'{1,2}'::INT[]",
        "SELECT '{}'::JSONB->1.5",
    ];
    let cycles = queries.map(|sql| fixture::execute_cycle(sql, None, 0, true));

    // Act
    let batches = fixture::run_cycles("jsonb-extract-invalid", &[], Vec::from(cycles));

    // Assert
    for (sql, frames) in queries.iter().zip(&batches) {
        assert_eq!(wire::error_code(frames).as_deref(), Some("42883"), "{sql}");
        assert!(wire::data_rows(frames).is_empty(), "{sql}");
    }
}

#[test]
fn should_reject_integer_array_constructors_given_jsonb_path_extraction_when_resolving_overloads() {
    // Arrange
    let cycle = fixture::execute_cycle("SELECT '{}'::JSONB#>ARRAY[1,2]", None, 0, true);
    // Act
    let batches = fixture::run_cycles("jsonb-extract-array-constructor-overload", &[], vec![cycle]);
    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42883"));
    assert_eq!(
        wire::data_rows(&batches[0]),
        Vec::<Vec<Option<String>>>::new()
    );
}
