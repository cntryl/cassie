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
