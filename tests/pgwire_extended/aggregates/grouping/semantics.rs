#[test]
fn should_reject_user_defined_collation_index_grouping_over_pgwire() {
    // Arrange
    let batches = run_script(
        "issue-796-user-collation-out-of-scope",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "CREATE COLLATION issue_796_ci_index (provider = icu, locale = 'und-u-ks-level2', deterministic = false)",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result).as_deref(), Some("0A000"));
}
