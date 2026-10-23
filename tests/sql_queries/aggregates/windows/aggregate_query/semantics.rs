#[test]
fn should_apply_lag_lead_window_arguments() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-lag-lead-offsets",
        &[
            "CREATE TABLE issue_806_lag_lead_values (id INT PRIMARY KEY, n INT)",
            "INSERT INTO issue_806_lag_lead_values VALUES (1,10),(2,20),(3,20),(4,40)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT id, LAG(n, 2, -1) OVER (ORDER BY n, id), LEAD(n, 2, -1) OVER (ORDER BY n, id) FROM issue_806_lag_lead_values ORDER BY n, id",
    );

    // Assert
    assert_eq!(
        result
            .expect("lag and lead offset/default arguments")
            .rows(),
        &[
            vec![Value::Int64(1), Value::Int64(-1), Value::Int64(20)],
            vec![Value::Int64(2), Value::Int64(-1), Value::Int64(40)],
            vec![Value::Int64(3), Value::Int64(10), Value::Int64(-1)],
            vec![Value::Int64(4), Value::Int64(20), Value::Int64(-1)],
        ]
    );
}

#[test]
fn should_return_tied_window_distribution_values() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-window-distributions",
        &[
            "CREATE TABLE issue_806_window_values (id INT PRIMARY KEY, n INT)",
            "INSERT INTO issue_806_window_values VALUES (1,10),(2,20),(3,20),(4,40)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT id, NTILE(2) OVER (ORDER BY n, id), PERCENT_RANK() OVER (ORDER BY n), CUME_DIST() OVER (ORDER BY n) FROM issue_806_window_values ORDER BY n, id",
    );

    // Assert
    assert_eq!(
        result.expect("window distribution values").rows(),
        &[
            vec![
                Value::Int64(1),
                Value::Int64(1),
                Value::Float64(0.0),
                Value::Float64(0.25)
            ],
            vec![
                Value::Int64(2),
                Value::Int64(1),
                Value::Float64(1.0 / 3.0),
                Value::Float64(0.75)
            ],
            vec![
                Value::Int64(3),
                Value::Int64(2),
                Value::Float64(1.0 / 3.0),
                Value::Float64(0.75)
            ],
            vec![
                Value::Int64(4),
                Value::Int64(2),
                Value::Float64(1.0),
                Value::Float64(1.0)
            ],
        ]
    );
}

#[test]
fn should_return_nth_value_for_an_explicit_full_partition_frame() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-nth-value-frame",
        &[
            "CREATE TABLE issue_806_nth_values (id INT PRIMARY KEY, n INT)",
            "INSERT INTO issue_806_nth_values VALUES (1,10),(2,20),(3,30)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT id, NTH_VALUE(n, 2) OVER (ORDER BY n ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING) FROM issue_806_nth_values ORDER BY id",
    );

    // Assert
    assert_eq!(
        result
            .expect("nth_value over the complete partition")
            .rows(),
        &[
            vec![Value::Int64(1), Value::Int64(20)],
            vec![Value::Int64(2), Value::Int64(20)],
            vec![Value::Int64(3), Value::Int64(20)],
        ]
    );
}

#[test]
fn should_keep_distinct_rows_frames_in_same_projection() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-rows-frame-offsets",
        &[
            "CREATE TABLE issue_806_rows_offsets (id INT)",
            "INSERT INTO issue_806_rows_offsets VALUES (1),(2),(3),(4)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT id, FIRST_VALUE(id) OVER (ORDER BY id ROWS BETWEEN 1 PRECEDING AND CURRENT ROW), FIRST_VALUE(id) OVER (ORDER BY id ROWS BETWEEN 1 FOLLOWING AND 2 FOLLOWING) FROM issue_806_rows_offsets ORDER BY id",
    );

    // Assert
    assert_eq!(
        result
            .expect("ROWS frame PRECEDING and FOLLOWING offsets")
            .rows(),
        &[
            vec![Value::Int64(1), Value::Int64(1), Value::Int64(2)],
            vec![Value::Int64(2), Value::Int64(1), Value::Int64(3)],
            vec![Value::Int64(3), Value::Int64(2), Value::Int64(4)],
            vec![Value::Int64(4), Value::Int64(3), Value::Null],
        ]
    );
}

#[test]
fn should_apply_frames_over_named_window_inheritance() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-named-window-inheritance",
        &[
            "CREATE TABLE issue_806_named_window (bucket TEXT, id INT, n INT)",
            "INSERT INTO issue_806_named_window VALUES ('a',1,2),('a',2,3),('b',1,5)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT bucket, id, SUM(n) OVER base, SUM(n) OVER (base ORDER BY id ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) FROM issue_806_named_window WINDOW base AS (PARTITION BY bucket) ORDER BY bucket, id",
    );

    // Assert
    assert_eq!(
        result.expect("named window inheritance").rows(),
        &[
            vec![
                Value::String("a".to_owned()),
                Value::Int64(1),
                Value::Int64(5),
                Value::Int64(2),
            ],
            vec![
                Value::String("a".to_owned()),
                Value::Int64(2),
                Value::Int64(5),
                Value::Int64(5),
            ],
            vec![
                Value::String("b".to_owned()),
                Value::Int64(1),
                Value::Int64(5),
                Value::Int64(5),
            ],
        ]
    );
}

#[test]
fn should_apply_groups_frames_with_each_frame_exclusion() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-groups-frame-exclusions",
        &[
            "CREATE TABLE issue_806_frame_values (n INT)",
            "INSERT INTO issue_806_frame_values VALUES (1),(2),(2),(3)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT n, FIRST_VALUE(n) OVER (ORDER BY n GROUPS BETWEEN 1 PRECEDING AND CURRENT ROW EXCLUDE NO OTHERS), FIRST_VALUE(n) OVER (ORDER BY n GROUPS BETWEEN 1 PRECEDING AND CURRENT ROW EXCLUDE CURRENT ROW), FIRST_VALUE(n) OVER (ORDER BY n GROUPS BETWEEN 1 PRECEDING AND CURRENT ROW EXCLUDE GROUP), FIRST_VALUE(n) OVER (ORDER BY n GROUPS BETWEEN 1 PRECEDING AND CURRENT ROW EXCLUDE TIES) FROM issue_806_frame_values ORDER BY n",
    );

    // Assert
    assert_eq!(
        result.expect("GROUPS frame exclusion semantics").rows(),
        &[
            vec![
                Value::Int64(1),
                Value::Int64(1),
                Value::Null,
                Value::Null,
                Value::Int64(1),
            ],
            vec![
                Value::Int64(2),
                Value::Int64(1),
                Value::Int64(1),
                Value::Int64(1),
                Value::Int64(1),
            ],
            vec![
                Value::Int64(2),
                Value::Int64(1),
                Value::Int64(1),
                Value::Int64(1),
                Value::Int64(1),
            ],
            vec![
                Value::Int64(3),
                Value::Int64(2),
                Value::Int64(2),
                Value::Int64(2),
                Value::Int64(2),
            ],
        ]
    );
}

#[test]
fn should_distinguish_exclude_group_from_exclude_ties_on_peer_rows() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-exclude-group-and-ties-peers",
        &[
            "CREATE TABLE issue_806_exclusion_peers (n INT, id INT)",
            "INSERT INTO issue_806_exclusion_peers VALUES (1,10),(2,20),(2,21),(3,30)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT id, FIRST_VALUE(id) OVER (ORDER BY n RANGE BETWEEN CURRENT ROW AND CURRENT ROW EXCLUDE GROUP), FIRST_VALUE(id) OVER (ORDER BY n RANGE BETWEEN CURRENT ROW AND CURRENT ROW EXCLUDE TIES) FROM issue_806_exclusion_peers ORDER BY n, id",
    );

    // Assert
    assert_eq!(
        result
            .expect("EXCLUDE GROUP removes peers while EXCLUDE TIES keeps the current row")
            .rows(),
        &[
            vec![Value::Int64(10), Value::Null, Value::Int64(10)],
            vec![Value::Int64(20), Value::Null, Value::Int64(20)],
            vec![Value::Int64(21), Value::Null, Value::Int64(21)],
            vec![Value::Int64(30), Value::Null, Value::Int64(30)],
        ]
    );
}

#[test]
fn should_apply_aggregate_filters_to_window_frames() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-aggregate-filter-window-frame",
        &[
            "CREATE TABLE issue_806_filtered_window (bucket TEXT, id INT, n INT)",
            "INSERT INTO issue_806_filtered_window VALUES ('a',1,2),('a',2,3),('b',1,5)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT bucket, id, SUM(n) FILTER (WHERE n > 2) OVER (PARTITION BY bucket ORDER BY id ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW), COUNT(*) FILTER (WHERE n > 2) OVER (PARTITION BY bucket) FROM issue_806_filtered_window ORDER BY bucket, id",
    );

    // Assert
    let result = result.expect("FILTER on ordered window aggregates");
    assert_eq!(
        result.rows(),
        &[
            vec![
                Value::String("a".to_owned()),
                Value::Int64(1),
                Value::Null,
                Value::Int64(1)
            ],
            vec![
                Value::String("a".to_owned()),
                Value::Int64(2),
                Value::Int64(3),
                Value::Int64(1)
            ],
            vec![
                Value::String("b".to_owned()),
                Value::Int64(1),
                Value::Int64(5),
                Value::Int64(1)
            ],
        ]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 23, 20, 20]
    );
}

#[test]
fn should_apply_boolean_aggregate_filters_over_ordered_window_frames() {
    // Arrange
    let fixture = sql_fixture(
        "issue-805-boolean-window-filters",
        &[
            "CREATE TABLE issue_805_boolean_window_values (bucket TEXT, id INT, flag BOOLEAN, selected BOOLEAN)",
            "INSERT INTO issue_805_boolean_window_values VALUES ('a',1,FALSE,TRUE),('a',2,TRUE,FALSE),('a',3,TRUE,TRUE),('a',4,NULL,TRUE),('b',1,TRUE,TRUE)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT bucket, id, BOOL_AND(flag) FILTER (WHERE selected) OVER (PARTITION BY bucket ORDER BY id ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW), BOOL_OR(flag) FILTER (WHERE selected) OVER (PARTITION BY bucket ORDER BY id ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) FROM issue_805_boolean_window_values ORDER BY bucket, id",
    );

    // Assert
    let result = result.expect("filtered boolean aggregates over ordered windows");
    assert_eq!(
        result.rows(),
        &[
            vec![
                Value::String("a".to_owned()),
                Value::Int64(1),
                Value::Bool(false),
                Value::Bool(false),
            ],
            vec![
                Value::String("a".to_owned()),
                Value::Int64(2),
                Value::Bool(false),
                Value::Bool(false),
            ],
            vec![
                Value::String("a".to_owned()),
                Value::Int64(3),
                Value::Bool(false),
                Value::Bool(true),
            ],
            vec![
                Value::String("a".to_owned()),
                Value::Int64(4),
                Value::Bool(false),
                Value::Bool(true),
            ],
            vec![
                Value::String("b".to_owned()),
                Value::Int64(1),
                Value::Bool(true),
                Value::Bool(true),
            ],
        ]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 23, 16, 16]
    );
}

#[test]
fn should_preserve_empty_results_for_filtered_window_aggregate_frames() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-filtered-window-aggregate-empty-frames",
        &[
            "CREATE TABLE issue_806_empty_filtered_window (id INT, n INT, flag BOOLEAN)",
            "INSERT INTO issue_806_empty_filtered_window VALUES (1,5,TRUE),(2,6,FALSE)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT id, \
         COUNT(*) FILTER (WHERE FALSE) OVER (ORDER BY id ROWS BETWEEN CURRENT ROW AND CURRENT ROW), \
         SUM(n) FILTER (WHERE FALSE) OVER (ORDER BY id ROWS BETWEEN CURRENT ROW AND CURRENT ROW), \
         BOOL_AND(flag) FILTER (WHERE FALSE) OVER (ORDER BY id ROWS BETWEEN CURRENT ROW AND CURRENT ROW), \
         BOOL_OR(flag) FILTER (WHERE FALSE) OVER (ORDER BY id ROWS BETWEEN CURRENT ROW AND CURRENT ROW) \
         FROM issue_806_empty_filtered_window ORDER BY id",
    );

    // Assert
    let result = result.expect("filtered window aggregates return empty-frame values");
    assert_eq!(
        result.rows(),
        &[
            vec![
                Value::Int64(1),
                Value::Int64(0),
                Value::Null,
                Value::Null,
                Value::Null,
            ],
            vec![
                Value::Int64(2),
                Value::Int64(0),
                Value::Null,
                Value::Null,
                Value::Null,
            ],
        ]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 20, 20, 16, 16]
    );
}

#[test]
fn should_skip_window_aggregate_arguments_when_the_filter_rejects_a_row() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-filter-before-window-aggregate-argument",
        &[
            "CREATE TABLE issue_806_filter_before_argument (id INT, value TEXT, selected BOOLEAN)",
            "INSERT INTO issue_806_filter_before_argument VALUES (1,'invalid',FALSE),(2,'2',TRUE)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT id, SUM(value::INT4) FILTER (WHERE selected) OVER ( \
             ORDER BY id ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW \
         ) FROM issue_806_filter_before_argument ORDER BY id",
    );

    // Assert
    let result = result.expect("filtered-out rows do not evaluate aggregate arguments");
    assert_eq!(
        result.rows(),
        &[
            vec![Value::Int64(1), Value::Null],
            vec![Value::Int64(2), Value::Int64(2)],
        ]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 20]
    );
}

#[test]
fn should_keep_window_results_out_of_wildcard_expansion() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-window-result-wildcard",
        &[
            "CREATE TABLE issue_806_window_wildcard (id INT, n INT)",
            "INSERT INTO issue_806_window_wildcard VALUES (1,10),(2,20)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT *, COUNT(*) OVER (ORDER BY id ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) FROM issue_806_window_wildcard ORDER BY id",
    );

    // Assert
    let result = result.expect("wildcard expansion excludes internal window values");
    assert_eq!(
        result.rows(),
        &[
            vec![Value::Int64(1), Value::Int64(10), Value::Int64(1)],
            vec![Value::Int64(2), Value::Int64(20), Value::Int64(2)],
        ]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 23, 20]
    );
}
