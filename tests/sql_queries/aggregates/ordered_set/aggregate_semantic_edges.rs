#[test]
fn should_choose_the_lowest_ordered_mode_when_frequencies_tie() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-mode-equal-frequency-tie",
        &[
            "CREATE TABLE issue_806_mode_tie_values (n INT)",
            "INSERT INTO issue_806_mode_tie_values VALUES (2),(1),(NULL)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT mode() WITHIN GROUP (ORDER BY n ASC), mode() WITHIN GROUP (ORDER BY n DESC) FROM issue_806_mode_tie_values",
    );

    // Assert
    assert_eq!(
        result
            .expect("mode resolves equal-frequency inputs by aggregate order")
            .rows(),
        &[vec![Value::Int64(1), Value::Int64(2)]]
    );
}

#[test]
fn should_rank_hypothetical_values_with_peer_null_rows() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-hypothetical-peer-null-ranking",
        &[
            "CREATE TABLE issue_806_hypothetical_peer_values (n INT)",
            "INSERT INTO issue_806_hypothetical_peer_values VALUES (1),(2),(2),(4),(NULL)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT rank(2) WITHIN GROUP (ORDER BY n), dense_rank(2) WITHIN GROUP (ORDER BY n), percent_rank(2) WITHIN GROUP (ORDER BY n), cume_dist(2) WITHIN GROUP (ORDER BY n) FROM issue_806_hypothetical_peer_values",
    );

    // Assert
    let result = result.expect("hypothetical ranks must handle peer and NULL rows");
    assert_eq!(
        result.rows(),
        &[vec![
            Value::Int64(2),
            Value::Int64(2),
            Value::Float64(0.2),
            Value::Float64(2.0 / 3.0),
        ]]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![20, 20, 701, 701]
    );
}

#[test]
fn should_include_boundary_fractions_in_ordered_set_percentiles() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-percentile-array-boundaries",
        &[
            "CREATE TABLE issue_806_percentile_boundary_values (n INT)",
            "INSERT INTO issue_806_percentile_boundary_values VALUES (1),(3),(NULL),(5),(7)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT percentile_cont(ARRAY[0.0,0.25,0.5,0.75,1.0]) WITHIN GROUP (ORDER BY n::FLOAT8)::TEXT, percentile_disc(ARRAY[0.0,0.25,0.5,0.75,1.0]) WITHIN GROUP (ORDER BY n)::TEXT FROM issue_806_percentile_boundary_values",
    );

    // Assert
    let result = result.expect("ordered-set percentiles include both boundary fractions");
    assert_eq!(
        result.rows(),
        &[vec![
            Value::String("{1,2.5,4,5.5,7}".to_owned()),
            Value::String("{1,1,3,5,7}".to_owned()),
        ]]
    );
}

#[test]
fn should_apply_ordered_set_empty_input_null_policies() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-ordered-set-empty-and-null-inputs",
        &[
            "CREATE TABLE issue_806_ordered_set_null_values (n INT)",
            "INSERT INTO issue_806_ordered_set_null_values VALUES (NULL),(NULL)",
        ],
    );
    let query = "SELECT mode() WITHIN GROUP (ORDER BY n), percentile_disc(0.5) WITHIN GROUP (ORDER BY n), percentile_cont(0.5) WITHIN GROUP (ORDER BY n::FLOAT8) FROM issue_806_ordered_set_null_values";

    // Act
    let all_null = fixture.execute(query);
    let empty = fixture.execute(&format!("{query} WHERE FALSE"));

    // Assert
    for result in [all_null, empty] {
        let result = result.expect("ordered-set aggregates return NULL without non-NULL inputs");
        assert_eq!(
            result.rows(),
            &[vec![Value::Null, Value::Null, Value::Null]]
        );
        assert_eq!(
            result
                .columns()
                .iter()
                .map(|column| column.type_oid)
                .collect::<Vec<_>>(),
            vec![23, 23, 701]
        );
    }
}

#[test]
fn should_filter_grouped_ordered_set_statistics_before_transitions() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-filtered-ordered-set-statistics",
        &[
            "CREATE TABLE issue_806_filtered_statistics (bucket TEXT, value FLOAT8, selected BOOLEAN)",
            "INSERT INTO issue_806_filtered_statistics VALUES ('kept',1,TRUE),('kept',3,TRUE),('kept',100,FALSE),('kept',5,TRUE),('empty',11,FALSE)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT bucket, PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY value) FILTER (WHERE selected), STDDEV_SAMP(value) FILTER (WHERE selected) FROM issue_806_filtered_statistics GROUP BY bucket ORDER BY bucket",
    );

    // Assert
    let result = result.expect("FILTER applies before ordered-set and statistical transitions");
    assert_eq!(
        result.rows(),
        &[
            vec![Value::String("empty".to_owned()), Value::Null, Value::Null],
            vec![
                Value::String("kept".to_owned()),
                Value::Float64(3.0),
                Value::Float64(2.0),
            ],
        ]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 701, 701]
    );
}
