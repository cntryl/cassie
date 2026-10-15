#[test]
fn should_report_malformed_json_text_sqlstate() {
    // Arrange
    let malformed_json = fixture::execute_cycle("SELECT '{bad}'::JSONB", None, 0, true);
    let recovery = fixture::execute_cycle("SELECT 42", None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-809-malformed-json-cast",
        &[],
        vec![malformed_json, recovery],
    );

    // Assert
    assert_error_without_result_rows(&batches[0], "22P02");
    assert_recovered_select(&batches[1]);
}

#[test]
fn should_reject_boolean_operand_for_jsonb_arrow_sqlstate() {
    // Arrange
    let wrong_operand = fixture::execute_cycle("SELECT '{\"a\":1}'::JSONB -> TRUE", None, 0, true);
    let recovery = fixture::execute_cycle("SELECT 42", None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-809-jsonb-arrow-invalid-operand",
        &[],
        vec![wrong_operand, recovery],
    );

    // Assert
    assert_error_without_result_rows(&batches[0], "42883");
    assert_recovered_select(&batches[1]);
}
