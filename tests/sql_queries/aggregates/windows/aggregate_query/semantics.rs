use cassie::types::extended::numeric::{math as numeric_math, parse_text, Numeric};

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
        "SELECT bucket, id, SUM(n) OVER base, SUM(n) OVER (base ORDER BY id ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW), SUM(n) OVER framed FROM issue_806_named_window WINDOW base AS (PARTITION BY bucket), framed AS (PARTITION BY bucket ORDER BY id ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) ORDER BY bucket, id",
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
                Value::Int64(2),
            ],
            vec![
                Value::String("a".to_owned()),
                Value::Int64(2),
                Value::Int64(5),
                Value::Int64(5),
                Value::Int64(5),
            ],
            vec![
                Value::String("b".to_owned()),
                Value::Int64(1),
                Value::Int64(5),
                Value::Int64(5),
                Value::Int64(5),
            ],
        ]
    );
}

#[test]
fn should_apply_chained_named_window_inheritance() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-chained-window-inheritance",
        &[
            "CREATE TABLE issue_806_chained_window (bucket TEXT, id INT, n INT)",
            "INSERT INTO issue_806_chained_window VALUES ('a',1,2),('a',2,3),('b',1,5)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT bucket, id, SUM(n) OVER framed, SUM(n) OVER suffix FROM issue_806_chained_window WINDOW base AS (PARTITION BY bucket), ordered AS (base ORDER BY id), framed AS (ordered ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW), suffix AS (ordered ROWS BETWEEN CURRENT ROW AND UNBOUNDED FOLLOWING) ORDER BY bucket, id",
    );

    // Assert
    assert_eq!(
        result.expect("chained named windows").rows(),
        &[
            vec![
                Value::String("a".to_owned()),
                Value::Int64(1),
                Value::Int64(2),
                Value::Int64(5),
            ],
            vec![
                Value::String("a".to_owned()),
                Value::Int64(2),
                Value::Int64(5),
                Value::Int64(3),
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
fn should_apply_frame_only_inheritance_from_an_empty_named_window() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-frame-only-window-inheritance",
        &[
            "CREATE TABLE issue_806_frame_only_window (n INT)",
            "INSERT INTO issue_806_frame_only_window VALUES (2)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT SUM(n) OVER framed FROM issue_806_frame_only_window WINDOW base AS (), framed AS (base ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW)",
    );

    // Assert
    assert_eq!(
        result.expect("frame-only extension of an empty base").rows(),
        &[vec![Value::Int64(2)]]
    );
}

#[test]
fn should_fold_unquoted_named_window_identifiers() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-window-name-folding",
        &[
            "CREATE TABLE issue_806_window_name_folding (bucket TEXT, n INT)",
            "INSERT INTO issue_806_window_name_folding VALUES ('a',2),('a',3)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT SUM(n) OVER base FROM issue_806_window_name_folding WINDOW Base AS (PARTITION BY bucket)",
    );

    // Assert
    assert_eq!(
        result.expect("unquoted window identifiers fold case").rows(),
        &[vec![Value::Int64(5)], vec![Value::Int64(5)]]
    );
}

#[test]
fn should_preserve_case_in_quoted_named_window_identifiers() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-quoted-window-name-case",
        &[
            "CREATE TABLE issue_806_quoted_window_name_case (n INT)",
            "INSERT INTO issue_806_quoted_window_name_case VALUES (2)",
        ],
    );

    // Act
    let exact_reference = fixture.execute(
        "SELECT SUM(n) OVER \"Base\" FROM issue_806_quoted_window_name_case WINDOW \"Base\" AS ()",
    );
    let folded_reference = fixture.execute(
        "SELECT SUM(n) OVER base FROM issue_806_quoted_window_name_case WINDOW \"Base\" AS ()",
    );

    // Assert
    assert_eq!(
        exact_reference
            .expect("quoted window name preserves case")
            .rows(),
        &[vec![Value::Int64(2)]]
    );
    assert!(
        folded_reference.is_err(),
        "an unquoted reference does not match a case-sensitive quoted name"
    );
}

#[test]
fn should_match_postgres_named_window_identifier_collisions() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-window-name-collisions",
        &[
            "CREATE TABLE issue_806_window_name_collisions (bucket TEXT, n INT)",
            "INSERT INTO issue_806_window_name_collisions VALUES ('a',2),('b',3)",
        ],
    );

    // Act
    let mixed_case_duplicate = fixture.execute(
        "SELECT SUM(n) OVER base FROM issue_806_window_name_collisions WINDOW Base AS (), base AS ()",
    );
    let quoted_lowercase_duplicate = fixture.execute(
        "SELECT SUM(n) OVER base FROM issue_806_window_name_collisions WINDOW \"base\" AS (), base AS ()",
    );
    let distinct_quoted_case = fixture.execute(
        "SELECT bucket, SUM(n) OVER \"Base\", SUM(n) OVER base FROM issue_806_window_name_collisions WINDOW \"Base\" AS (PARTITION BY bucket), base AS () ORDER BY bucket",
    );

    // Assert
    assert!(mixed_case_duplicate.is_err());
    assert!(quoted_lowercase_duplicate.is_err());
    assert_eq!(
        distinct_quoted_case
            .expect("quoted uppercase and folded lowercase names are distinct")
            .rows(),
        &[
            vec![
                Value::String("a".to_owned()),
                Value::Int64(2),
                Value::Int64(5),
            ],
            vec![
                Value::String("b".to_owned()),
                Value::Int64(3),
                Value::Int64(5),
            ],
        ]
    );
}

#[test]
fn should_reject_window_clause_after_order_by() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-window-clause-order",
        &["CREATE TABLE issue_806_window_clause_order (id INT, n INT)"],
    );

    // Act
    let result = fixture.execute(
        "SELECT SUM(n) OVER base FROM issue_806_window_clause_order ORDER BY id WINDOW base AS ()",
    );

    // Assert
    assert!(result.is_err(), "WINDOW must precede ORDER BY");
}

#[test]
fn should_accept_window_clause_after_having_before_pagination() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-window-clause-position",
        &[
            "CREATE TABLE issue_806_window_clause_position (bucket TEXT, n INT)",
            "INSERT INTO issue_806_window_clause_position VALUES ('a',1),('a',2),('b',3)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT bucket, COUNT(*) OVER base FROM issue_806_window_clause_position WHERE n > 0 GROUP BY bucket HAVING COUNT(*) > 0 WINDOW base AS (PARTITION BY bucket) ORDER BY bucket LIMIT 2 OFFSET 0",
    );

    // Assert
    assert_eq!(
        result.expect("WINDOW follows HAVING and precedes pagination").rows(),
        &[
            vec![Value::String("a".to_owned()), Value::Int64(1)],
            vec![Value::String("b".to_owned()), Value::Int64(1)],
        ]
    );
}

#[test]
fn should_reject_having_after_window_clause() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-having-after-window",
        &[
            "CREATE TABLE issue_806_having_after_window (bucket TEXT, n INT)",
            "INSERT INTO issue_806_having_after_window VALUES ('a',1)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT bucket, COUNT(*) OVER base FROM issue_806_having_after_window GROUP BY bucket WINDOW base AS (PARTITION BY bucket) HAVING COUNT(*) > 0",
    );

    // Assert
    assert!(result.is_err(), "WINDOW must follow HAVING");
}

#[test]
fn should_reject_copying_a_framed_named_window() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-framed-window-copy",
        &[
            "CREATE TABLE issue_806_framed_window_copy (id INT, n INT)",
            "INSERT INTO issue_806_framed_window_copy VALUES (1,2),(2,3)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT SUM(n) OVER (base) FROM issue_806_framed_window_copy WINDOW base AS (ORDER BY id ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW)",
    );

    // Assert
    assert!(result.is_err(), "a framed window cannot be copied");
}

#[test]
fn should_reject_adding_partition_by_to_an_inherited_window() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-inherited-partition",
        &[
            "CREATE TABLE issue_806_inherited_partition (bucket TEXT, id INT, n INT)",
            "INSERT INTO issue_806_inherited_partition VALUES ('a',1,2),('a',2,3)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT SUM(n) OVER (base PARTITION BY bucket) FROM issue_806_inherited_partition WINDOW base AS ()",
    );

    // Assert
    assert!(result.is_err(), "an inherited window cannot add PARTITION BY");
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

#[test]
fn should_evaluate_statistical_window_frames() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-statistical-window-frames",
        &[
            "CREATE TABLE issue_806_statistical_window_values \
             (id INT, n FLOAT8, exact_n NUMERIC)",
            "INSERT INTO issue_806_statistical_window_values \
             VALUES (1,1,1),(2,3,3),(3,5,5)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT id, \
         VAR_POP(n) OVER (ORDER BY id ROWS BETWEEN 1 PRECEDING AND CURRENT ROW), \
         VAR_SAMP(n) OVER (ORDER BY id ROWS BETWEEN 1 PRECEDING AND CURRENT ROW), \
         VARIANCE(n) OVER (ORDER BY id ROWS BETWEEN 1 PRECEDING AND CURRENT ROW), \
         STDDEV_POP(n) OVER (ORDER BY id ROWS BETWEEN 1 PRECEDING AND CURRENT ROW), \
         STDDEV_SAMP(n) OVER (ORDER BY id ROWS BETWEEN 1 PRECEDING AND CURRENT ROW), \
         STDDEV(n) OVER (ORDER BY id ROWS BETWEEN 1 PRECEDING AND CURRENT ROW), \
         VAR_POP(exact_n) OVER (ORDER BY id ROWS BETWEEN 1 PRECEDING AND 1 PRECEDING), \
         STDDEV_SAMP(exact_n) OVER (ORDER BY id ROWS BETWEEN 1 PRECEDING AND CURRENT ROW) \
         FROM issue_806_statistical_window_values ORDER BY id",
    );

    // Assert
    let result = result.expect("variance and standard deviation over ROWS frames");
    let float_rows = result
        .rows()
        .iter()
        .map(|row| row[..7].to_vec())
        .collect::<Vec<_>>();
    assert_eq!(
        float_rows,
        vec![
            vec![
                Value::Int64(1),
                Value::Float64(0.0),
                Value::Null,
                Value::Null,
                Value::Float64(0.0),
                Value::Null,
                Value::Null,
            ],
            vec![
                Value::Int64(2),
                Value::Float64(1.0),
                Value::Float64(2.0),
                Value::Float64(2.0),
                Value::Float64(1.0),
                Value::Float64(2.0_f64.sqrt()),
                Value::Float64(2.0_f64.sqrt()),
            ],
            vec![
                Value::Int64(3),
                Value::Float64(1.0),
                Value::Float64(2.0),
                Value::Float64(2.0),
                Value::Float64(1.0),
                Value::Float64(2.0_f64.sqrt()),
                Value::Float64(2.0_f64.sqrt()),
            ],
        ]
    );
    assert_eq!(result.rows()[0][7..], [Value::Null, Value::Null]);
    for row in &result.rows()[1..] {
        let Value::String(actual) = &row[7] else {
            panic!("NUMERIC window result should retain text carrier, got {:?}", row[7]);
        };
        let actual = parse_text(actual).expect("parse NUMERIC population variance");
        assert_eq!(
            actual.compare(&Numeric::zero()),
            std::cmp::Ordering::Equal
        );
    }
    let expected_numeric_stddev =
        numeric_math::sqrt(&Numeric::from_i128(2)).expect("numeric sample standard deviation");
    for row in &result.rows()[1..] {
        let Value::String(actual) = &row[8] else {
            panic!("NUMERIC window result should retain text carrier, got {:?}", row[8]);
        };
        let actual = parse_text(actual).expect("parse NUMERIC window result");
        assert_eq!(
            actual.compare(&expected_numeric_stddev),
            std::cmp::Ordering::Equal
        );
    }
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 701, 701, 701, 701, 701, 701, 1700, 1700]
    );
}
