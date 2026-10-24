use super::*;

#[test]
fn should_match_jsonb_scalar_nested_containment_rules() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT '[1,2,3]'::JSONB @> '2'::JSONB, \
         '2'::JSONB @> '2.0'::JSONB, \
         '[{\"a\":1,\"b\":2}]'::JSONB @> '{\"a\":1,\"b\":2}'::JSONB, \
         '{\"a\":{\"b\":1,\"c\":2}}'::JSONB @> '{\"a\":{\"b\":1}}'::JSONB, \
         '{\"a\":[{\"b\":1,\"c\":2}]}'::JSONB \
             @> '{\"a\":[{\"b\":1}]}'::JSONB, \
         '[[1]]'::JSONB @> '[1]'::JSONB, \
         '[1]'::JSONB @> '[[1]]'::JSONB",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-containment-shapes", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONB containment RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16; 7]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
            Some("f".to_owned()),
        ]]
    );
}

#[test]
fn should_evaluate_jsonb_contained_by_as_reverse_containment() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT '[3,1]'::JSONB <@ '[1,2,3]'::JSONB, \
         '{\"a\":1}'::JSONB <@ '{\"a\":1,\"b\":2}'::JSONB, \
         '2'::JSONB <@ '[1,2,3]'::JSONB, \
         '[2]'::JSONB <@ '2'::JSONB",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-contained-by", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
        ]]
    );
}

#[test]
fn should_infer_jsonb_containment_parameter_type() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT '[1,2,3]'::JSONB @> $1",
        Some((0, 0, Some(b"2"))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-containment-parameter", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let parameter_types = frames
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("JSONB containment parameter description");
    assert_eq!(wire::parse_parameter_description(&parameter_types.1), [114]);
    fixture::assert_single_column_descriptors(frames, (16, 1, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![Some("t".to_owned())]]);
}

#[test]
fn should_propagate_sql_null_through_jsonb_containment() {
    // Arrange
    let cycle = fixture::execute_cycle("SELECT NULL::JSONB @> 'null'::JSONB", None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-containment-null", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (16, 1, -1), 0, true);
    assert_eq!(wire::data_rows(frames), vec![vec![None]]);
}

#[test]
fn should_reject_incompatible_jsonb_containment_operands() {
    // Arrange
    let cycle = fixture::execute_cycle("SELECT '{}'::JSONB @> 1", None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-containment-invalid", &[], vec![cycle]);

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42883"));
}

#[test]
fn should_preserve_inet_containment_semantics() {
    // Arrange
    let cycle = fixture::execute_cycle(
        "SELECT '192.168.1.0/24'::INET @> '192.168.1.0/25'::INET",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-inet-containment", &[], vec![cycle]);

    // Assert
    fixture::assert_success(&batches[0]);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("t".to_owned())]]
    );
}
