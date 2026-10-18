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
fn should_resolve_output_alias_inside_grouping_sets() {
    // Arrange
    let fixture = grouping_fixture("issue-804-grouping-set-alias");

    // Act
    let result = fixture.execute(
        "SELECT region AS area, SUM(amount), GROUPING(region) FROM issue_804_group_values GROUP BY GROUPING SETS ((area), ()) ORDER BY GROUPING(region), area NULLS FIRST",
    );

    // Assert
    assert_eq!(
        result
            .expect("GROUP BY output alias inside grouping sets")
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
}

#[test]
fn should_resolve_output_ordinal_inside_grouping_sets() {
    // Arrange
    let fixture = grouping_fixture("issue-804-grouping-set-ordinal");

    // Act
    let result = fixture.execute(
        "SELECT region AS area, SUM(amount), GROUPING(region) FROM issue_804_group_values GROUP BY GROUPING SETS ((1), ()) ORDER BY GROUPING(region), region NULLS FIRST",
    );

    // Assert
    assert_eq!(
        result
            .expect("GROUP BY output ordinal inside grouping sets")
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
}

#[test]
fn should_return_grouping_sets_indicators_as_int4_with_sum_results() {
    // Arrange
    let fixture = grouping_fixture("issue-804-grouping-sets-indicator-metadata");

    // Act
    let result = fixture.execute(
        "SELECT region, GROUPING(region), SUM(amount) FROM issue_804_group_values GROUP BY GROUPING SETS ((region), ()) ORDER BY GROUPING(region), region NULLS FIRST",
    );

    // Assert
    let result = result.expect("ordinary aggregate values and GROUPING metadata for grouping sets");
    assert_eq!(
        result.rows(),
        &[
            vec![
                Value::String("east".to_owned()),
                Value::Int64(0),
                Value::Int64(5),
            ],
            vec![
                Value::String("west".to_owned()),
                Value::Int64(0),
                Value::Int64(5),
            ],
            vec![Value::Null, Value::Int64(1), Value::Int64(10)],
        ]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 23, 20]
    );
}
