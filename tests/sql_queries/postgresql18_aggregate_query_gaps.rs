//! PostgreSQL 18 aggregate and window contracts that remain outside current focused coverage.

use cassie::types::Value;

use super::super::support_sql_fixture::{self, sql_fixture};

fn grouping_fixture(label: &str) -> support_sql_fixture::SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE issue_804_group_values (region TEXT, product TEXT, amount INT, label TEXT, id INT PRIMARY KEY)",
            "INSERT INTO issue_804_group_values VALUES ('east','book',2,'e',1),('east','pen',3,'e',2),('west','book',5,'w',3)",
        ],
    )
}

#[test]
fn should_expand_rollup_across_detail_subtotal_total_rows() {
    // Arrange
    let fixture = grouping_fixture("issue-804-rollup-grouping");

    // Act
    let result = fixture.execute(
        "SELECT region, product, SUM(amount), GROUPING(region, product) FROM issue_804_group_values GROUP BY ROLLUP(region, product) ORDER BY GROUPING(region, product), region NULLS FIRST, product NULLS FIRST",
    );

    // Assert
    assert_eq!(
        result.expect("ROLLUP grouping rows and indicators").rows(),
        &[
            vec![
                Value::String("east".to_owned()),
                Value::String("book".to_owned()),
                Value::Int64(2),
                Value::Int64(0),
            ],
            vec![
                Value::String("east".to_owned()),
                Value::String("pen".to_owned()),
                Value::Int64(3),
                Value::Int64(0),
            ],
            vec![
                Value::String("west".to_owned()),
                Value::String("book".to_owned()),
                Value::Int64(5),
                Value::Int64(0),
            ],
            vec![
                Value::String("east".to_owned()),
                Value::Null,
                Value::Int64(5),
                Value::Int64(1),
            ],
            vec![
                Value::String("west".to_owned()),
                Value::Null,
                Value::Int64(5),
                Value::Int64(1),
            ],
            vec![Value::Null, Value::Null, Value::Int64(10), Value::Int64(3)],
        ]
    );
}

#[test]
fn should_expand_cube_into_each_grouping_set() {
    // Arrange
    let fixture = grouping_fixture("issue-804-cube-grouping");

    // Act
    let result = fixture.execute(
        "SELECT region, product, SUM(amount), GROUPING(region, product) FROM issue_804_group_values GROUP BY CUBE(region, product) ORDER BY GROUPING(region, product), region NULLS FIRST, product NULLS FIRST",
    );

    // Assert
    assert_eq!(
        result.expect("CUBE grouping rows and indicators").rows(),
        &[
            vec![
                Value::String("east".to_owned()),
                Value::String("book".to_owned()),
                Value::Int64(2),
                Value::Int64(0),
            ],
            vec![
                Value::String("east".to_owned()),
                Value::String("pen".to_owned()),
                Value::Int64(3),
                Value::Int64(0),
            ],
            vec![
                Value::String("west".to_owned()),
                Value::String("book".to_owned()),
                Value::Int64(5),
                Value::Int64(0),
            ],
            vec![
                Value::String("east".to_owned()),
                Value::Null,
                Value::Int64(5),
                Value::Int64(1),
            ],
            vec![
                Value::String("west".to_owned()),
                Value::Null,
                Value::Int64(5),
                Value::Int64(1),
            ],
            vec![
                Value::Null,
                Value::String("book".to_owned()),
                Value::Int64(7),
                Value::Int64(2),
            ],
            vec![
                Value::Null,
                Value::String("pen".to_owned()),
                Value::Int64(3),
                Value::Int64(2),
            ],
            vec![Value::Null, Value::Null, Value::Int64(10), Value::Int64(3)],
        ]
    );
}

#[test]
fn should_expand_mixed_nested_grouping_sets_with_duplicates() {
    // Arrange
    let fixture = grouping_fixture("issue-804-nested-mixed-grouping-sets");

    // Act
    let result = fixture.execute(
        "SELECT region, product, SUM(amount), GROUPING(region, product) FROM issue_804_group_values GROUP BY GROUPING SETS (ROLLUP(region, product), CUBE(product)) ORDER BY GROUPING(region, product), region NULLS FIRST, product NULLS FIRST",
    );

    // Assert
    assert_eq!(
        result.expect("mixed nested grouping sets").rows(),
        &[
            vec![
                Value::String("east".to_owned()),
                Value::String("book".to_owned()),
                Value::Int64(2),
                Value::Int64(0),
            ],
            vec![
                Value::String("east".to_owned()),
                Value::String("pen".to_owned()),
                Value::Int64(3),
                Value::Int64(0),
            ],
            vec![
                Value::String("west".to_owned()),
                Value::String("book".to_owned()),
                Value::Int64(5),
                Value::Int64(0),
            ],
            vec![
                Value::String("east".to_owned()),
                Value::Null,
                Value::Int64(5),
                Value::Int64(1),
            ],
            vec![
                Value::String("west".to_owned()),
                Value::Null,
                Value::Int64(5),
                Value::Int64(1),
            ],
            vec![
                Value::Null,
                Value::String("book".to_owned()),
                Value::Int64(7),
                Value::Int64(2),
            ],
            vec![
                Value::Null,
                Value::String("pen".to_owned()),
                Value::Int64(3),
                Value::Int64(2),
            ],
            vec![Value::Null, Value::Null, Value::Int64(10), Value::Int64(3)],
            vec![Value::Null, Value::Null, Value::Int64(10), Value::Int64(3)],
        ]
    );
}

#[test]
fn should_distinguish_distinct_from_all_duplicate_grouping_sets() {
    // Arrange
    let fixture = grouping_fixture("issue-804-distinct-all-grouping-sets");

    // Act
    let distinct = fixture.execute(
        "SELECT region, SUM(amount), GROUPING(region) FROM issue_804_group_values GROUP BY DISTINCT GROUPING SETS ((region), (region), ()) ORDER BY GROUPING(region), region NULLS FIRST",
    );
    let all = fixture.execute(
        "SELECT region, SUM(amount), GROUPING(region) FROM issue_804_group_values GROUP BY ALL GROUPING SETS ((region), (region), ()) ORDER BY GROUPING(region), region NULLS FIRST",
    );

    // Assert
    assert_eq!(
        distinct
            .expect("duplicate grouping sets with DISTINCT")
            .rows(),
        &[
            vec![
                Value::String("east".to_owned()),
                Value::Int64(5),
                Value::Int64(0),
            ],
            vec![
                Value::String("west".to_owned()),
                Value::Int64(5),
                Value::Int64(0),
            ],
            vec![Value::Null, Value::Int64(10), Value::Int64(1)],
        ]
    );
    assert_eq!(
        all.expect("duplicate grouping sets with ALL").rows(),
        &[
            vec![
                Value::String("east".to_owned()),
                Value::Int64(5),
                Value::Int64(0),
            ],
            vec![
                Value::String("east".to_owned()),
                Value::Int64(5),
                Value::Int64(0),
            ],
            vec![
                Value::String("west".to_owned()),
                Value::Int64(5),
                Value::Int64(0),
            ],
            vec![
                Value::String("west".to_owned()),
                Value::Int64(5),
                Value::Int64(0),
            ],
            vec![Value::Null, Value::Int64(10), Value::Int64(1)],
        ]
    );
}

#[test]
fn should_apply_explicit_all_to_nullable_count_sum() {
    // Arrange
    let fixture = sql_fixture(
        "issue-804-all-aggregate-arguments",
        &[
            "CREATE TABLE issue_804_all_aggregate_values (bucket TEXT, amount INT)",
            "INSERT INTO issue_804_all_aggregate_values VALUES ('a',2),('a',2),('a',NULL),('b',NULL)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT bucket, COUNT(ALL amount), SUM(ALL amount) FROM issue_804_all_aggregate_values GROUP BY bucket ORDER BY bucket",
    );

    // Assert
    let result = result.expect("explicit ALL aggregate arguments");
    assert_eq!(
        result.rows(),
        &[
            vec![
                Value::String("a".to_owned()),
                Value::Int64(2),
                Value::Int64(4),
            ],
            vec![Value::String("b".to_owned()), Value::Int64(0), Value::Null,],
        ]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 20, 20]
    );
}

#[test]
fn should_project_primary_key_functional_dependencies_with_grouped_aggregates() {
    // Arrange
    let fixture = grouping_fixture("issue-804-grouping-functional-dependency");

    // Act
    let result = fixture.execute(
        "SELECT id, label, SUM(amount) FROM issue_804_group_values GROUP BY id ORDER BY id",
    );

    // Assert
    assert_eq!(
        result.expect("primary-key dependent projection").rows(),
        &[
            vec![
                Value::Int64(1),
                Value::String("e".to_owned()),
                Value::Int64(2)
            ],
            vec![
                Value::Int64(2),
                Value::String("e".to_owned()),
                Value::Int64(3)
            ],
            vec![
                Value::Int64(3),
                Value::String("w".to_owned()),
                Value::Int64(5)
            ],
        ]
    );
}

#[test]
fn should_apply_filter_before_boolean_aggregate_transitions() {
    // Arrange
    let fixture = sql_fixture(
        "issue-805-filtered-boolean-aggregates",
        &[
            "CREATE TABLE issue_805_bool_values (bucket TEXT, flag BOOLEAN, selected BOOLEAN)",
            "INSERT INTO issue_805_bool_values VALUES ('mixed',TRUE,TRUE),('mixed',FALSE,TRUE),('mixed',NULL,FALSE),('none',TRUE,FALSE),('nulls',NULL,TRUE),('true',TRUE,TRUE),('true',TRUE,TRUE),('false',FALSE,TRUE)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT bucket, BOOL_AND(flag) FILTER (WHERE selected), BOOL_OR(flag) FILTER (WHERE selected), COUNT(flag) FILTER (WHERE selected) FROM issue_805_bool_values GROUP BY bucket ORDER BY bucket",
    );

    // Assert
    let result = result.expect("filtered boolean aggregates by group");
    assert_eq!(
        result.rows(),
        &[
            vec![
                Value::String("false".to_owned()),
                Value::Bool(false),
                Value::Bool(false),
                Value::Int64(1),
            ],
            vec![
                Value::String("mixed".to_owned()),
                Value::Bool(false),
                Value::Bool(true),
                Value::Int64(2),
            ],
            vec![
                Value::String("none".to_owned()),
                Value::Null,
                Value::Null,
                Value::Int64(0),
            ],
            vec![
                Value::String("nulls".to_owned()),
                Value::Null,
                Value::Null,
                Value::Int64(0),
            ],
            vec![
                Value::String("true".to_owned()),
                Value::Bool(true),
                Value::Bool(true),
                Value::Int64(2),
            ],
        ]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 16, 16, 20]
    );
}

#[test]
fn should_return_postgres_bitwise_aggregate_values() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-bitwise-aggregates",
        &[
            "CREATE TABLE issue_806_bitwise_values (n INT)",
            "INSERT INTO issue_806_bitwise_values VALUES (1),(2),(3),(4)",
        ],
    );
    let cases = [
        (
            "SELECT bit_and(n) FROM issue_806_bitwise_values",
            Value::Int64(0),
        ),
        (
            "SELECT bit_or(n) FROM issue_806_bitwise_values",
            Value::Int64(7),
        ),
        (
            "SELECT bit_xor(n) FROM issue_806_bitwise_values",
            Value::Int64(4),
        ),
    ];

    // Act
    let failures = cases
        .iter()
        .filter_map(|(sql, expected)| match fixture.execute(sql) {
            Ok(result) if result.rows() == [vec![expected.clone()]] => None,
            Ok(result) => Some(format!(
                "{sql}: expected {expected:?}, got {:?}",
                result.rows()
            )),
            Err(error) => Some(format!("{sql}: {error}")),
        })
        .collect::<Vec<_>>();

    // Assert
    assert!(
        failures.is_empty(),
        "bitwise aggregates differ:\n{}",
        failures.join("\n")
    );
}

#[test]
fn should_return_every_for_boolean_inputs_with_nulls() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-every-aggregate",
        &[
            "CREATE TABLE issue_806_every_values (flag BOOLEAN)",
            "INSERT INTO issue_806_every_values VALUES (TRUE),(TRUE),(FALSE),(NULL)",
        ],
    );

    // Act
    let result = fixture.execute("SELECT EVERY(flag) FROM issue_806_every_values");

    // Assert
    assert_eq!(
        result
            .expect("EVERY ignores NULL and returns false for a false input")
            .rows(),
        &[vec![Value::Bool(false)]]
    );
}

#[test]
fn should_interpolate_continuous_percentiles_after_null_filtering() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-percentile-interpolation",
        &[
            "CREATE TABLE issue_806_percentile_values (n INT)",
            "INSERT INTO issue_806_percentile_values VALUES (1),(3),(NULL),(5),(7)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT PERCENTILE_CONT(0.25) WITHIN GROUP (ORDER BY n::FLOAT8), PERCENTILE_DISC(0.25) WITHIN GROUP (ORDER BY n), COUNT(n) FROM issue_806_percentile_values",
    );

    // Assert
    let result = result.expect("continuous and discrete percentiles with a NULL input");
    assert_eq!(
        result.rows(),
        &[vec![Value::Float64(2.5), Value::Int64(1), Value::Int64(4)]]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![701, 23, 20]
    );
}

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
fn should_return_any_non_null_input_from_any_value_aggregate() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-any-value-aggregate",
        &[
            "CREATE TABLE issue_806_any_values (bucket INT, item INT)",
            "INSERT INTO issue_806_any_values VALUES (1,10),(1,20),(2,NULL)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT bucket, any_value(item), COUNT(item) FROM issue_806_any_values GROUP BY bucket ORDER BY bucket",
    );

    // Assert
    let result = result.expect("ANY_VALUE over grouped inputs");
    assert_eq!(result.rows().len(), 2);
    let first = &result.rows()[0];
    assert_eq!(first[0], Value::Int64(1));
    assert!(
        matches!(first[1], Value::Int64(10 | 20)),
        "ANY_VALUE may select any non-NULL input, got {:?}",
        first[1]
    );
    assert_eq!(first[2], Value::Int64(2));
    assert_eq!(
        result.rows()[1],
        vec![Value::Int64(2), Value::Null, Value::Int64(0)]
    );
}

#[test]
fn should_group_by_a_select_output_alias() {
    // Arrange
    let fixture = grouping_fixture("issue-804-group-by-alias");

    // Act
    let by_alias = fixture.execute(
        "SELECT region AS area, SUM(amount) FROM issue_804_group_values GROUP BY area ORDER BY area",
    );

    // Assert
    assert_eq!(
        by_alias.expect("GROUP BY output alias").rows(),
        &[
            vec![Value::String("east".to_owned()), Value::Int64(5)],
            vec![Value::String("west".to_owned()), Value::Int64(5)],
        ]
    );
}

#[test]
fn should_group_by_a_select_list_ordinal() {
    // Arrange
    let fixture = grouping_fixture("issue-804-group-by-ordinal");

    // Act
    let by_ordinal = fixture
        .execute("SELECT region, SUM(amount) FROM issue_804_group_values GROUP BY 1 ORDER BY 1");

    // Assert
    assert_eq!(
        by_ordinal.expect("GROUP BY output ordinal").rows(),
        &[
            vec![Value::String("east".to_owned()), Value::Int64(5)],
            vec![Value::String("west".to_owned()), Value::Int64(5)],
        ]
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
fn should_return_statistical_regression_aggregate_values() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-statistical-regression-aggregates",
        &[
            "CREATE TABLE issue_806_regression_values (x INT, y INT)",
            "INSERT INTO issue_806_regression_values VALUES (1,2),(2,4),(3,6)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT corr(CAST(x AS FLOAT8), CAST(y AS FLOAT8)), covar_pop(CAST(x AS FLOAT8), CAST(y AS FLOAT8)), covar_samp(CAST(x AS FLOAT8), CAST(y AS FLOAT8)), regr_count(CAST(y AS FLOAT8), CAST(x AS FLOAT8)), regr_slope(CAST(y AS FLOAT8), CAST(x AS FLOAT8)), regr_intercept(CAST(y AS FLOAT8), CAST(x AS FLOAT8)), regr_r2(CAST(y AS FLOAT8), CAST(x AS FLOAT8)), regr_sxx(CAST(y AS FLOAT8), CAST(x AS FLOAT8)), regr_sxy(CAST(y AS FLOAT8), CAST(x AS FLOAT8)), regr_syy(CAST(y AS FLOAT8), CAST(x AS FLOAT8)), regr_avgx(CAST(y AS FLOAT8), CAST(x AS FLOAT8)), regr_avgy(CAST(y AS FLOAT8), CAST(x AS FLOAT8)) FROM issue_806_regression_values",
    );

    // Assert
    assert_eq!(
        result
            .expect("statistical and regression aggregates")
            .rows(),
        &[vec![
            Value::Float64(1.0),
            Value::Float64(4.0 / 3.0),
            Value::Float64(2.0),
            Value::Int64(3),
            Value::Float64(2.0),
            Value::Float64(0.0),
            Value::Float64(1.0),
            Value::Float64(2.0),
            Value::Float64(4.0),
            Value::Float64(8.0),
            Value::Float64(2.0),
            Value::Float64(4.0),
        ]]
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
    assert_eq!(
        result.expect("FILTER on ordered window aggregates").rows(),
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
}

#[test]
fn should_skip_sql_nulls_but_keep_json_nulls_in_strict_jsonb_aggregate() {
    // Arrange
    let fixture = sql_fixture(
        "issue-811-jsonb-agg-strict-null-policy",
        &[
            "CREATE TABLE issue_811_strict_json_values (payload JSONB)",
            "INSERT INTO issue_811_strict_json_values VALUES (NULL), ('null')",
        ],
    );
    let sql = "SELECT jsonb_agg_strict(payload)::TEXT FROM issue_811_strict_json_values";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("strict JSONB aggregate null policy").rows(),
        &[vec![Value::String("[null]".to_owned())]]
    );
}
