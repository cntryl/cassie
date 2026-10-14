//! Focused PostgreSQL 18 aggregate edge cases beyond the broader owner suites.

use cassie::types::Value;

use super::super::support_sql_fixture::sql_fixture;

mod hypothetical_multicolumn;

#[test]
fn should_distinguish_source_nulls_from_rollup_nulls() {
    // Arrange
    let fixture = sql_fixture(
        "issue-804-rollup-source-null-distinction",
        &[
            "CREATE TABLE issue_804_rollup_null_values (region TEXT, product TEXT, amount INT)",
            "INSERT INTO issue_804_rollup_null_values VALUES (NULL,'book',4),('east','book',2),('east',NULL,3),(NULL,NULL,1)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT region, product, SUM(amount), GROUPING(region, product) FROM issue_804_rollup_null_values GROUP BY ROLLUP(region, product) ORDER BY GROUPING(region, product), region NULLS FIRST, product NULLS FIRST",
    );

    // Assert
    let result = result.expect("ROLLUP must distinguish source NULLs from rolled-up NULLs");
    assert_eq!(
        result.rows(),
        &[
            vec![Value::Null, Value::Null, Value::Int64(1), Value::Int64(0)],
            vec![
                Value::Null,
                Value::String("book".to_owned()),
                Value::Int64(4),
                Value::Int64(0),
            ],
            vec![
                Value::String("east".to_owned()),
                Value::Null,
                Value::Int64(3),
                Value::Int64(0),
            ],
            vec![
                Value::String("east".to_owned()),
                Value::String("book".to_owned()),
                Value::Int64(2),
                Value::Int64(0),
            ],
            vec![Value::Null, Value::Null, Value::Int64(5), Value::Int64(1)],
            vec![
                Value::String("east".to_owned()),
                Value::Null,
                Value::Int64(5),
                Value::Int64(1),
            ],
            vec![Value::Null, Value::Null, Value::Int64(10), Value::Int64(3)],
        ]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 25, 20, 23]
    );
}

#[test]
fn should_preserve_filtered_distinct_aggregates_in_grouping_sets() {
    // Arrange
    let fixture = sql_fixture(
        "issue-804-filtered-distinct-grouping-sets",
        &[
            "CREATE TABLE issue_804_filtered_distinct_values (bucket TEXT, amount INT, selected BOOLEAN)",
            "INSERT INTO issue_804_filtered_distinct_values VALUES ('north',4,TRUE),('north',4,TRUE),('north',4,FALSE),('north',10,TRUE),('south',2,FALSE),('south',2,NULL),('east',NULL,TRUE)",
        ],
    );
    let grouped_query = "SELECT bucket, GROUPING(bucket), COUNT(*) FILTER (WHERE selected), COUNT(DISTINCT amount) FILTER (WHERE selected), SUM(DISTINCT amount) FILTER (WHERE selected), STRING_AGG(DISTINCT amount::TEXT, ',' ORDER BY amount::TEXT DESC) FILTER (WHERE selected) FROM issue_804_filtered_distinct_values GROUP BY GROUPING SETS ((bucket), ()) ORDER BY GROUPING(bucket), bucket NULLS FIRST";
    let having_query = "SELECT bucket, GROUPING(bucket), COUNT(*) FILTER (WHERE selected), COUNT(DISTINCT amount) FILTER (WHERE selected), SUM(DISTINCT amount) FILTER (WHERE selected), STRING_AGG(DISTINCT amount::TEXT, ',' ORDER BY amount::TEXT DESC) FILTER (WHERE selected) FROM issue_804_filtered_distinct_values GROUP BY ROLLUP(bucket) HAVING COUNT(*) FILTER (WHERE selected) > 1 ORDER BY GROUPING(bucket), bucket NULLS FIRST";

    // Act
    let grouped = fixture.execute(grouped_query);
    let having = fixture.execute(having_query);

    // Assert
    let grouped = grouped.expect("filtered distinct aggregates in grouping sets");
    assert_eq!(
        grouped.rows(),
        &[
            vec![
                Value::String("east".to_owned()),
                Value::Int64(0),
                Value::Int64(1),
                Value::Int64(0),
                Value::Null,
                Value::Null,
            ],
            vec![
                Value::String("north".to_owned()),
                Value::Int64(0),
                Value::Int64(3),
                Value::Int64(2),
                Value::Int64(14),
                Value::String("4,10".to_owned()),
            ],
            vec![
                Value::String("south".to_owned()),
                Value::Int64(0),
                Value::Int64(0),
                Value::Int64(0),
                Value::Null,
                Value::Null,
            ],
            vec![
                Value::Null,
                Value::Int64(1),
                Value::Int64(4),
                Value::Int64(2),
                Value::Int64(14),
                Value::String("4,10".to_owned()),
            ],
        ]
    );
    assert_eq!(
        grouped
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 23, 20, 20, 20, 25]
    );

    let having = having.expect("HAVING evaluates the filtered aggregate per grouping set");
    assert_eq!(
        having.rows(),
        &[
            vec![
                Value::String("north".to_owned()),
                Value::Int64(0),
                Value::Int64(3),
                Value::Int64(2),
                Value::Int64(14),
                Value::String("4,10".to_owned()),
            ],
            vec![
                Value::Null,
                Value::Int64(1),
                Value::Int64(4),
                Value::Int64(2),
                Value::Int64(14),
                Value::String("4,10".to_owned()),
            ],
        ]
    );
    assert_eq!(
        having
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 23, 20, 20, 20, 25]
    );
}

fn assert_float_near(value: &Value, expected: f64) {
    let Value::Float64(actual) = value else {
        panic!("expected a float value, got {value:?}");
    };
    assert!(
        (*actual - expected).abs() < 1e-12,
        "expected {expected:?}, got {actual:?}"
    );
}

#[test]
fn should_return_null_for_boolean_aggregate_without_nonnull_inputs() {
    // Arrange
    let fixture = sql_fixture(
        "issue-805-empty-null-boolean-aggregates",
        &[
            "CREATE TABLE issue_805_boolean_null_inputs (flag BOOLEAN)",
            "INSERT INTO issue_805_boolean_null_inputs VALUES (NULL), (NULL)",
        ],
    );

    // Act
    let all_null = fixture.execute(
        "SELECT BOOL_AND(flag), BOOL_OR(flag), EVERY(flag) FROM issue_805_boolean_null_inputs",
    );
    let empty = fixture.execute(
        "SELECT BOOL_AND(flag), BOOL_OR(flag), EVERY(flag) \
         FROM issue_805_boolean_null_inputs WHERE FALSE",
    );

    // Assert
    for result in [all_null, empty] {
        let result = result.expect("boolean aggregates over no non-NULL inputs");
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
            vec![16, 16, 16]
        );
    }
}

#[test]
fn should_accept_distinct_boolean_aggregate_inputs() {
    // Arrange
    let fixture = sql_fixture(
        "issue-805-distinct-boolean-aggregates",
        &[
            "CREATE TABLE issue_805_boolean_distinct (flag BOOLEAN)",
            "INSERT INTO issue_805_boolean_distinct VALUES (TRUE),(TRUE),(FALSE),(FALSE),(NULL)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT BOOL_AND(DISTINCT flag), BOOL_OR(DISTINCT flag) FROM issue_805_boolean_distinct",
    );

    // Assert
    let result = result.expect("DISTINCT Boolean aggregates over duplicate and NULL inputs");
    assert_eq!(
        result.rows(),
        &[vec![Value::Bool(false), Value::Bool(true)]]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16, 16]
    );
}

#[test]
fn should_filter_regression_rows_pairwise_when_either_input_is_null() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-regression-pair-null-filter",
        &[
            "CREATE TABLE issue_806_regression_pair_values (x INT, y INT)",
            "INSERT INTO issue_806_regression_pair_values VALUES (1,2),(2,6),(NULL,10),(4,NULL),(5,10)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT corr(CAST(x AS FLOAT), CAST(y AS FLOAT)), covar_pop(CAST(x AS FLOAT), CAST(y AS FLOAT)), covar_samp(CAST(x AS FLOAT), CAST(y AS FLOAT)), regr_count(CAST(y AS FLOAT), CAST(x AS FLOAT)), regr_slope(CAST(y AS FLOAT), CAST(x AS FLOAT)), regr_intercept(CAST(y AS FLOAT), CAST(x AS FLOAT)), regr_r2(CAST(y AS FLOAT), CAST(x AS FLOAT)), regr_sxx(CAST(y AS FLOAT), CAST(x AS FLOAT)), regr_sxy(CAST(y AS FLOAT), CAST(x AS FLOAT)), regr_syy(CAST(y AS FLOAT), CAST(x AS FLOAT)), regr_avgx(CAST(y AS FLOAT), CAST(x AS FLOAT)), regr_avgy(CAST(y AS FLOAT), CAST(x AS FLOAT)) FROM issue_806_regression_pair_values",
    );

    // Assert
    let result = result.expect("statistical aggregates must share pairwise NULL filtering");
    let row = &result.rows()[0];
    assert_float_near(&row[0], 0.960_768_922_830_522_8);
    assert_float_near(&row[1], 16.0 / 3.0);
    assert_float_near(&row[2], 8.0);
    assert_eq!(row[3], Value::Int64(3));
    assert_float_near(&row[4], 24.0 / 13.0);
    assert_float_near(&row[5], 14.0 / 13.0);
    assert_float_near(&row[6], 12.0 / 13.0);
    assert_float_near(&row[7], 26.0 / 3.0);
    assert_float_near(&row[8], 16.0);
    assert_float_near(&row[9], 32.0);
    assert_float_near(&row[10], 8.0 / 3.0);
    assert_float_near(&row[11], 6.0);
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![701, 701, 701, 20, 701, 701, 701, 701, 701, 701, 701, 701]
    );
}

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
    let result =
        fixture.execute("SELECT mode() WITHIN GROUP (ORDER BY n) FROM issue_806_mode_tie_values");

    // Assert
    assert_eq!(
        result
            .expect("mode resolves equal-frequency inputs by aggregate order")
            .rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_skip_nulls_in_bitwise_aggregates() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-bitwise-empty-and-null-inputs",
        &[
            "CREATE TABLE issue_806_bitwise_null_values (n INT)",
            "INSERT INTO issue_806_bitwise_null_values VALUES (NULL),(NULL)",
        ],
    );

    // Act
    let all_null = fixture.execute(
        "SELECT bit_and(n), bit_or(n), bit_xor(n), COUNT(n) FROM issue_806_bitwise_null_values",
    );
    let empty = fixture.execute(
        "SELECT bit_and(n), bit_or(n), bit_xor(n), COUNT(n) FROM issue_806_bitwise_null_values WHERE FALSE",
    );

    // Assert
    let expected = &[vec![Value::Null, Value::Null, Value::Null, Value::Int64(0)]];
    for result in [all_null, empty] {
        let result = result.expect("bitwise aggregates ignore NULL and return NULL without values");
        assert_eq!(result.rows(), expected);
        assert_eq!(
            result
                .columns()
                .iter()
                .map(|column| column.type_oid)
                .collect::<Vec<_>>(),
            vec![23, 23, 23, 20]
        );
    }
}

#[test]
fn should_return_population_zero_for_singleton_statistics() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-statistical-singleton",
        &[
            "CREATE TABLE issue_806_statistical_singleton_values (n FLOAT)",
            "INSERT INTO issue_806_statistical_singleton_values VALUES (7),(NULL)",
        ],
    );

    // Act
    let singleton = fixture.execute(
        "SELECT var_pop(n), var_samp(n), stddev_pop(n), stddev_samp(n) FROM issue_806_statistical_singleton_values WHERE n = 7",
    );
    let empty = fixture.execute(
        "SELECT var_pop(n), var_samp(n), stddev_pop(n), stddev_samp(n) FROM issue_806_statistical_singleton_values WHERE FALSE",
    );

    // Assert
    let singleton = singleton.expect("sample and population statistics over one value");
    assert_eq!(
        singleton.rows(),
        &[vec![
            Value::Float64(0.0),
            Value::Null,
            Value::Float64(0.0),
            Value::Null,
        ]]
    );
    let empty = empty.expect("sample and population statistics over no values");
    assert_eq!(
        empty.rows(),
        &[vec![Value::Null, Value::Null, Value::Null, Value::Null]]
    );
    assert_eq!(
        singleton
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![701, 701, 701, 701]
    );
}

#[test]
fn should_distinguish_sample_from_population_statistics_for_multiple_values() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-statistical-sample-population-denominators",
        &[
            "CREATE TABLE issue_806_statistical_denominators (n FLOAT8)",
            "INSERT INTO issue_806_statistical_denominators VALUES (1), (3), (5), (NULL)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT var_pop(n), var_samp(n), stddev_pop(n), stddev_samp(n) \
         FROM issue_806_statistical_denominators",
    );

    // Assert
    let result = result.expect("sample and population statistics over multiple values");
    assert_eq!(result.rows().len(), 1);
    let row = &result.rows()[0];
    assert_float_near(&row[0], 8.0 / 3.0);
    assert_float_near(&row[1], 4.0);
    assert_float_near(&row[2], (8.0_f64 / 3.0).sqrt());
    assert_float_near(&row[3], 2.0);
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![701; 4]
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

#[test]
fn should_filter_distinct_array_aggregation_by_ordered_inputs() {
    // Arrange
    let fixture = sql_fixture(
        "issue-811-array-aggregate-distinct-order-filter",
        &[
            "CREATE TABLE issue_811_ordered_array_values (position INT, selected BOOLEAN, value INT)",
            "INSERT INTO issue_811_ordered_array_values VALUES (1,TRUE,2),(2,FALSE,99),(3,TRUE,NULL),(4,TRUE,1),(5,TRUE,2),(6,TRUE,NULL)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT ARRAY_AGG(DISTINCT value ORDER BY value DESC) FILTER (WHERE selected)::TEXT FROM issue_811_ordered_array_values",
    );

    // Assert
    let result = result.expect("array_agg filters before DISTINCT and aggregate-local ordering");
    assert_eq!(
        result.rows(),
        &[vec![Value::String("{NULL,2,1}".to_owned())]]
    );
    assert_eq!(result.columns()[0].type_oid, 25);
}

#[test]
fn should_order_json_collection_aggregate_inputs() {
    // Arrange
    let fixture = sql_fixture(
        "issue-811-ordered-json-aggregate-strictness",
        &[
            "CREATE TABLE issue_811_ordered_json_values (position INT, payload JSONB)",
            "INSERT INTO issue_811_ordered_json_values VALUES (1,'2'),(2,NULL),(3,'null'),(4,'1'),(5,'2')",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT JSON_AGG(payload ORDER BY position)::TEXT, JSONB_AGG(payload ORDER BY position)::TEXT FROM issue_811_ordered_json_values",
    );

    // Assert
    let result = result.expect("JSON collection aggregates preserve aggregate-local order");
    assert_eq!(
        result.rows(),
        &[vec![
            Value::String("[2, null, null, 1, 2]".to_owned()),
            Value::String("[2, null, null, 1, 2]".to_owned()),
        ]]
    );
}

#[test]
fn should_omit_sql_nulls_from_ordered_strict_json_aggregates() {
    // Arrange
    let fixture = sql_fixture(
        "issue-811-ordered-strict-json-aggregates",
        &[
            "CREATE TABLE issue_811_ordered_strict_json_values (position INT, payload JSONB)",
            "INSERT INTO issue_811_ordered_strict_json_values VALUES (1,'2'),(2,NULL),(3,'null'),(4,'1'),(5,'2')",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT JSON_AGG_STRICT(payload ORDER BY position)::TEXT, JSONB_AGG_STRICT(payload ORDER BY position)::TEXT FROM issue_811_ordered_strict_json_values",
    );

    // Assert
    let result = result.expect("strict JSON aggregates preserve order and omit SQL NULL inputs");
    assert_eq!(
        result.rows(),
        &[vec![
            Value::String("[2, null, 1, 2]".to_owned()),
            Value::String("[2, null, 1, 2]".to_owned()),
        ]]
    );
}

#[test]
fn should_apply_postgres_null_policy_to_strict_json_object_aggregates() {
    // Arrange
    let fixture = sql_fixture(
        "issue-811-strict-json-object-aggregate-null-policy",
        &[
            "CREATE TABLE issue_811_strict_object_values (key TEXT, json_value JSON, jsonb_value JSONB)",
            "INSERT INTO issue_811_strict_object_values VALUES ('skip',NULL,NULL),('keep','null','null'),('other','1','1')",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT JSON_OBJECT_AGG_STRICT(key,json_value ORDER BY key)::TEXT, JSONB_OBJECT_AGG_STRICT(key,jsonb_value ORDER BY key)::TEXT, JSON_OBJECT_AGG_UNIQUE_STRICT(key,json_value ORDER BY key)::TEXT, JSONB_OBJECT_AGG_UNIQUE_STRICT(key,jsonb_value ORDER BY key)::TEXT FROM issue_811_strict_object_values",
    );

    // Assert
    // PostgreSQL 18.6 preserves a JSON null value for JSON aggregates while
    // JSONB strict object aggregates omit JSONB null values.
    let result =
        result.expect("strict JSON object aggregates follow PostgreSQL 18.6 null behavior");
    assert_eq!(
        result.rows(),
        &[vec![
            Value::String("{ \"keep\" : null, \"other\" : 1 }".to_owned()),
            Value::String("{\"other\": 1}".to_owned()),
            Value::String("{ \"keep\" : null, \"other\" : 1 }".to_owned()),
            Value::String("{\"other\": 1}".to_owned()),
        ]]
    );
}

#[test]
fn should_apply_exclusion_modes_to_grouped_window_aggregates() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-grouped-window-aggregate-exclusion",
        &[
            "CREATE TABLE issue_806_grouped_window_values (score INT, amount INT)",
            "INSERT INTO issue_806_grouped_window_values VALUES (1,10),(2,20),(2,30),(3,40)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT score, amount, SUM(amount) OVER (ORDER BY score GROUPS BETWEEN CURRENT ROW AND CURRENT ROW EXCLUDE TIES), SUM(amount) OVER (ORDER BY score GROUPS BETWEEN CURRENT ROW AND CURRENT ROW EXCLUDE GROUP), SUM(amount) OVER (ORDER BY score GROUPS BETWEEN CURRENT ROW AND CURRENT ROW EXCLUDE CURRENT ROW) FROM issue_806_grouped_window_values ORDER BY score, amount",
    );

    // Assert
    assert_eq!(
        result
            .expect("window aggregates apply each exclusion mode to peer rows")
            .rows(),
        &[
            vec![
                Value::Int64(1),
                Value::Int64(10),
                Value::Int64(10),
                Value::Null,
                Value::Null,
            ],
            vec![
                Value::Int64(2),
                Value::Int64(20),
                Value::Int64(20),
                Value::Null,
                Value::Int64(30),
            ],
            vec![
                Value::Int64(2),
                Value::Int64(30),
                Value::Int64(30),
                Value::Null,
                Value::Int64(20),
            ],
            vec![
                Value::Int64(3),
                Value::Int64(40),
                Value::Int64(40),
                Value::Null,
                Value::Null,
            ],
        ]
    );
}

#[test]
fn should_apply_each_exclusion_mode_to_rows_window_aggregates() {
    // Arrange
    let fixture = sql_fixture(
        "issue-806-rows-frame-aggregate-exclusions",
        &[
            "CREATE TABLE issue_806_rows_exclusion_values (score INT, amount INT)",
            "INSERT INTO issue_806_rows_exclusion_values VALUES (1,10),(2,20),(2,30),(3,40)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT score, amount, \
         SUM(amount) OVER (ORDER BY score ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING EXCLUDE NO OTHERS), \
         SUM(amount) OVER (ORDER BY score ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING EXCLUDE CURRENT ROW), \
         SUM(amount) OVER (ORDER BY score ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING EXCLUDE GROUP), \
         SUM(amount) OVER (ORDER BY score ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING EXCLUDE TIES) \
         FROM issue_806_rows_exclusion_values ORDER BY score, amount",
    );

    // Assert
    assert_eq!(
        result
            .expect("ROWS frames apply each exclusion mode to duplicate peers")
            .rows(),
        &[
            vec![
                Value::Int64(1),
                Value::Int64(10),
                Value::Int64(100),
                Value::Int64(90),
                Value::Int64(90),
                Value::Int64(100),
            ],
            vec![
                Value::Int64(2),
                Value::Int64(20),
                Value::Int64(100),
                Value::Int64(80),
                Value::Int64(50),
                Value::Int64(70),
            ],
            vec![
                Value::Int64(2),
                Value::Int64(30),
                Value::Int64(100),
                Value::Int64(70),
                Value::Int64(50),
                Value::Int64(80),
            ],
            vec![
                Value::Int64(3),
                Value::Int64(40),
                Value::Int64(100),
                Value::Int64(60),
                Value::Int64(60),
                Value::Int64(100),
            ],
        ]
    );
}
