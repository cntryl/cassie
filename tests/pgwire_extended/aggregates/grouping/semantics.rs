#[test]
fn should_preserve_nondeterministic_collation_for_equality_grouping_unique_indexes() {
    // Arrange
    let batches = run_script(
        "issue-796-nondeterministic-collation-index-grouping",
        vec![
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE COLLATION issue_796_ci_index (provider = icu, locale = 'und-u-ks-level2', deterministic = false)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_796_ci_values (value TEXT COLLATE issue_796_ci_index)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_796_ci_values VALUES ('A'), ('a'), ('b')",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE INDEX issue_796_ci_values_value_idx ON issue_796_ci_values (value)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT count(*) FROM issue_796_ci_values WHERE value = 'a'",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "SELECT count(*) FROM issue_796_ci_values GROUP BY value ORDER BY count(*) DESC",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE TABLE issue_796_unique_ci_values (value TEXT COLLATE issue_796_ci_index)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "CREATE UNIQUE INDEX issue_796_unique_ci_values_value_idx ON issue_796_unique_ci_values (value)",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_796_unique_ci_values VALUES ('A')",
            )]),
            Step::ThroughReady(vec![wire::simple_query_frame(
                "INSERT INTO issue_796_unique_ci_values VALUES ('a')",
            )]),
        ],
    );

    // Act
    let setup = &batches[..4];
    let indexed_equality = &batches[4];
    let grouped = &batches[5];
    let unique_index_setup = &batches[6..9];
    let duplicate = &batches[9];

    // Assert
    let setup_errors = setup
        .iter()
        .flat_map(|frames| {
            frames
                .iter()
                .filter(|frame| frame.0 == b'E')
                .map(|frame| wire::parse_error_fields(&frame.1))
        })
        .collect::<Vec<_>>();
    assert!(
        setup
            .iter()
            .all(|frames| wire::error_code(frames).is_none()),
        "collation and index setup errors: {setup_errors:?}"
    );
    assert_eq!(wire::error_code(indexed_equality), None);
    assert_eq!(
        wire::data_rows(indexed_equality),
        vec![vec![Some("2".into())]]
    );
    assert_eq!(wire::error_code(grouped), None);
    assert_eq!(
        wire::data_rows(grouped),
        vec![vec![Some("2".into())], vec![Some("1".into())]]
    );
    assert!(
        unique_index_setup
            .iter()
            .all(|frames| wire::error_code(frames).is_none()),
        "unique collation index setup errors: {:?}",
        unique_index_setup
            .iter()
            .flat_map(|frames| {
                frames
                    .iter()
                    .filter(|frame| frame.0 == b'E')
                    .map(|frame| wire::parse_error_fields(&frame.1))
            })
            .collect::<Vec<_>>()
    );
    assert_eq!(wire::error_code(duplicate).as_deref(), Some("23505"));
}
