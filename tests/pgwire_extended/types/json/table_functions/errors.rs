#[test]
fn should_abort_json_recordset_on_a_later_typed_conversion_error_without_partial_rows() {
    // Arrange
    let sql = "SELECT id, n FROM jsonb_to_recordset('[{\"id\":1,\"n\":1},{\"id\":2,\"n\":\"bad\"}]'::jsonb) AS records(id INTEGER, n INTEGER)";
    let cycles = vec![
        fixture::execute_cycle(sql, None, 0, true),
        fixture::execute_cycle("SELECT 42", None, 0, true),
    ];

    // Act
    let batches = fixture::run_cycles("type-json-recordset-later-conversion-error", &[], cycles);

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22P02"));
    assert_eq!(
        wire::data_rows(&batches[0]),
        Vec::<Vec<Option<String>>>::new()
    );
    fixture::assert_success(&batches[1]);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("42".to_owned())]]
    );
}
