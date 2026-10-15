#[test]
fn should_expand_parameterized_array_with_ordinality_from_text_bind() {
    // Arrange
    let cycle = fixture::execute_cycle(
        PARAMETERIZED_UNNEST_SQL,
        Some((1_007, 0, Some(b"[0:2]={7,NULL,9}"))),
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-808-parameterized-unnest-text-bind", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_ordinality_descriptors(frames, 0);
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![Some("7".to_owned()), Some("1".to_owned())],
            vec![None, Some("2".to_owned())],
            vec![Some("9".to_owned()), Some("3".to_owned())],
        ]
    );
}
