
#[test]
fn should_expand_jsonb_object_keys_as_text_rows() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_jsonb_keys_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_jsonb_keys_values VALUES ('{\"b\":2,\"a\":1}')",
            Vec::new(),
        ),
    ];
    let sql = "SELECT jsonb_object_keys(document) FROM issue_810_jsonb_keys_values ORDER BY 1";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-810-jsonb-object-keys", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    fixture::assert_single_column_descriptors(frames, (25, -1, -1), 0, true);
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("a".to_owned())], vec![Some("b".to_owned())]]
    );
}
