#[test]
fn should_return_multiple_fraction_percentiles_as_arrays() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-percentile-fraction-arrays",
        &[
            "CREATE TABLE issue_806_percentile_array_values (n INT)",
            "INSERT INTO issue_806_percentile_array_values VALUES (1),(2),(3),(4)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT PERCENTILE_CONT(ARRAY[0.25, 0.5, 1.0]) WITHIN GROUP (ORDER BY n::FLOAT8)::TEXT, PERCENTILE_DISC(ARRAY[0.25, 0.5, 1.0]) WITHIN GROUP (ORDER BY n)::TEXT FROM issue_806_percentile_array_values",
    );

    // Assert
    assert_eq!(
        result
            .expect("array-fraction ordered-set percentiles")
            .rows(),
        &[vec![
            Value::String("{1.75,2.5,4}".to_owned()),
            Value::String("{1,2,4}".to_owned()),
        ]]
    );
}

#[test]
fn should_interpolate_continuous_percentiles_for_interval_values() {
    // Arrange
    let fixture = sql_fixture("issue-806-percentile-interval-values", &[]);

    // Act
    let result =
        fixture.execute("SELECT PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY INTERVAL '1 hour')");

    // Assert
    assert_eq!(
        result.expect("continuous percentile over intervals").rows(),
        &[vec![Value::String("01:00:00".to_owned())]]
    );
}

#[test]
fn should_return_ordered_set_mode_for_the_most_frequent_input() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-mode-ordered-set",
        &[
            "CREATE TABLE issue_806_mode_values (n INT)",
            "INSERT INTO issue_806_mode_values VALUES (1),(1),(2),(3)",
        ],
    );

    // Act
    let result =
        fixture.execute("SELECT mode() WITHIN GROUP (ORDER BY n) FROM issue_806_mode_values");

    // Assert
    assert_eq!(
        result.expect("ordered-set mode").rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_rank_hypothetical_rows_inside_ordered_sets() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-hypothetical-set-ranks",
        &[
            "CREATE TABLE issue_806_hypothetical_values (n INT)",
            "INSERT INTO issue_806_hypothetical_values VALUES (1),(3)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT rank(2) WITHIN GROUP (ORDER BY n), dense_rank(2) WITHIN GROUP (ORDER BY n), percent_rank(2) WITHIN GROUP (ORDER BY n), cume_dist(2) WITHIN GROUP (ORDER BY n) FROM issue_806_hypothetical_values",
    );

    // Assert
    assert_eq!(
        result.expect("hypothetical-set ranks").rows(),
        &[vec![
            Value::Int64(2),
            Value::Int64(2),
            Value::Float64(0.5),
            Value::Float64(2.0 / 3.0),
        ]]
    );
}
