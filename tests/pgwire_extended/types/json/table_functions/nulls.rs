#[test]
fn should_preserve_json_null_when_expanding_jsonb_array_elements_text() {
    // Arrange
    let sql = "SELECT value FROM jsonb_array_elements_text('[null,\"a\",\"c\"]'::jsonb) AS value ORDER BY value NULLS FIRST";
    let cycles = vec![fixture::execute_cycle(sql, None, 0, true)];

    // Act
    let batches = fixture::run_cycles("issue-829-jsonb-array-elements-text", &[], cycles);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSON array-element RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![None],
            vec![Some("a".to_owned())],
            vec![Some("c".to_owned())],
        ]
    );
}
