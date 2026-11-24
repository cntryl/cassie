use super::support_relational_evidence::SeededRelationalFixture;

#[test]
fn should_preserve_seeded_relational_results_under_supported_rewrites() {
    // Arrange
    let forward = SeededRelationalFixture::from_seed(0x51_11CE, false).execute();
    let reverse = SeededRelationalFixture::from_seed(0x51_11CE, true).execute();

    // Act
    let join_rewrite = (&forward.keyed_join, &forward.filtered_cross_join);
    let cte_rewrite = (&forward.cte, &forward.direct);

    // Assert
    assert_eq!(join_rewrite.0, join_rewrite.1);
    assert_eq!(cte_rewrite.0, cte_rewrite.1);
    assert_eq!(forward.keyed_join, reverse.keyed_join);
    assert_eq!(forward.cte, reverse.cte);
    assert_eq!(forward.window, reverse.window);
    assert_eq!(forward.window, forward.expected_window);
    assert!(forward.has_null_window_value);
    assert!(forward.has_tied_window_rank);
    assert_eq!(forward.aggregate, forward.expected_aggregate);
    assert_eq!(forward.aggregate, reverse.aggregate);
}
