#[test]
fn should_detect_jsonb_object_key_presence_without_other_operators() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_key_presence_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_key_presence_values VALUES ('{\"present\":null}')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT document?'present', document ? 'missing' FROM issue_809_key_presence_values",
        None,
        0,
        true,
    );
    let parameter = fixture::execute_cycle(
        "SELECT '{\"present\":null}'::JSONB ? $1",
        Some((0, 0, Some(b"present"))),
        0,
        true,
    );
    let invalid_operand = fixture::execute_cycle(
        "SELECT document ? TRUE FROM issue_809_key_presence_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles(
        "issue-809-jsonb-key-presence",
        &setup,
        vec![cycle, parameter, invalid_operand],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("t".to_owned()), Some("f".to_owned())]]
    );
    let parameter_frames = &batches[1];
    fixture::assert_success(parameter_frames);
    let parameter_types = parameter_frames
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("text operator ParameterDescription");
    assert_eq!(wire::parse_parameter_description(&parameter_types.1), [25]);
    assert_eq!(wire::data_rows(parameter_frames), vec![vec![Some("t".to_owned())]]);
    assert_eq!(wire::error_code(&batches[2]).as_deref(), Some("42883"));
}

