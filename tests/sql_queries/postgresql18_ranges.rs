//! PostgreSQL 18 range and multirange value behavior tracked by #905.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_canonicalize_builtin_ranges_with_adjacent_multiranges() {
    // Arrange
    let fixture = sql_fixture("issue-905-range-canonicalization", &[]);
    let sql = "SELECT '[1,4]'::int4range = '[1,5)'::int4range, '[1.20,2.40)'::numrange = numrange(1.2, 2.4, '[)'), 'empty'::int8range = int8range(1, 1, '[)'), daterange('2020-01-01', '2020-01-03', '[]') = '[2020-01-01,2020-01-04)'::daterange, '{[1,3),[3,5)}'::int4multirange = '{[1,5)}'::int4multirange";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("range canonicalization").rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
        ]]
    );
}

#[test]
fn should_handle_unbounded_empty_inclusive_range_boundaries() {
    // Arrange
    let fixture = sql_fixture("issue-905-range-boundaries", &[]);
    let sql = "SELECT lower_inf('(,)'::daterange) AND upper_inf('(,)'::daterange), int4range(1, 5, '[)') @> 1, int4range(1, 5, '[)') @> 4, NOT (int4range(1, 5, '[)') @> 5), isempty('empty'::int8range), isempty(int8range(1, 1, '[)'))";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("range boundary truth table").rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
        ]]
    );
}

#[test]
fn should_intersect_merge_builtin_ranges() {
    // Arrange
    let fixture = sql_fixture("issue-905-range-intersection-hull", &[]);
    let sql = "SELECT (int4range(1, 5, '[)') * int4range(3, 7, '[)'))::TEXT, range_merge(int4range(1, 5, '[)'), int4range(3, 7, '[)'))::TEXT";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("range intersection and hull").rows(),
        &[vec![
            Value::String("[3,5)".to_owned()),
            Value::String("[1,7)".to_owned()),
        ]]
    );
}

#[test]
fn should_distinguish_overlapping_adjacent_separated_ranges() {
    // Arrange
    let fixture = sql_fixture("issue-905-range-relations", &[]);
    let sql = "SELECT int4range(1,3,'[)') && int4range(2,4,'[)'), int4range(1,3,'[)') -|- int4range(3,5,'[)'), int4range(1,3,'[)') && int4range(3,5,'[)'), int4range(1,3,'[)') -|- int4range(4,6,'[)'), int4range(1,3,'[)') < int4range(2,4,'[)')";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("range overlap, adjacency, and order").rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(false),
            Value::Bool(true),
        ]]
    );
}

#[test]
fn should_apply_range_multirange_set_operators() {
    // Arrange
    let fixture = sql_fixture("issue-905-range-multirange-set-operators", &[]);
    let sql = "SELECT (int4range(1,3,'[)') + int4range(3,5,'[)'))::text, (int4range(1,7,'[)') - int4range(3,7,'[)'))::text, ('{[1,3),[5,7)}'::int4multirange + '{[3,5)}'::int4multirange)::text, ('{[1,7)}'::int4multirange - '{[3,5)}'::int4multirange)::text, ('{[1,3),[5,7)}'::int4multirange * '{[2,6)}'::int4multirange)::text";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("range and multirange set operators").rows(),
        &[vec![
            Value::String("[1,5)".to_owned()),
            Value::String("[1,3)".to_owned()),
            Value::String("{[1,7)}".to_owned()),
            Value::String("{[1,3),[5,7)}".to_owned()),
            Value::String("{[2,3),[5,6)}".to_owned()),
        ]]
    );
}

#[test]
fn should_inspect_finite_discrete_continuous_range_bounds() {
    // Arrange
    let fixture = sql_fixture("issue-905-finite-range-bounds", &[]);
    let sql = "SELECT lower('[2,7)'::int4range) = 2, upper('[2,7]'::int4range) = 8, lower_inc('[2,7]'::int4range) AND NOT upper_inc('[2,7]'::int4range), lower('(2,7]'::int4range) = 3, lower_inc('[2,7]'::numrange) AND upper_inc('[2,7]'::numrange)";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("finite range bounds and inclusivity").rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
        ]]
    );
}

#[test]
fn should_evaluate_timestamp_ranges_by_local_value_or_instant() {
    // Arrange
    let fixture = sql_fixture("issue-905-timestamp-range-values", &[]);
    let sql = "SELECT tsrange(TIMESTAMP '2024-03-10 01:30:00', TIMESTAMP '2024-03-10 03:30:00', '[)') @> TIMESTAMP '2024-03-10 02:30:00', tsrange(TIMESTAMP '2024-03-10 01:30:00', TIMESTAMP '2024-03-10 03:30:00', '[)') && tsrange(TIMESTAMP '2024-03-10 03:00:00', TIMESTAMP '2024-03-10 04:00:00', '[)'), NOT (tsrange(TIMESTAMP '2024-03-10 01:30:00', TIMESTAMP '2024-03-10 03:30:00', '[)') @> TIMESTAMP '2024-03-10 03:30:00'), tstzrange(TIMESTAMPTZ '2024-03-10 01:30:00-05', TIMESTAMPTZ '2024-03-10 03:30:00-04', '[)') = tstzrange(TIMESTAMPTZ '2024-03-10 06:30:00+00', TIMESTAMPTZ '2024-03-10 07:30:00+00', '[)'), tstzrange(TIMESTAMPTZ '2024-03-10 01:30:00-05', TIMESTAMPTZ '2024-03-10 03:30:00-04', '[)') @> TIMESTAMPTZ '2024-03-10 07:00:00+00'";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result
            .expect("timestamp and timestamptz range values")
            .rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
        ]]
    );
}

#[test]
fn should_normalize_values_across_remaining_multirange_families() {
    // Arrange
    let fixture = sql_fixture("issue-905-multirange-family-values", &[]);
    let sql = "SELECT '{[1,3),[3,5)}'::int8multirange = '{[1,5)}'::int8multirange, '{[1.20,2.40),[2.40,3.60)}'::nummultirange = '{[1.20,3.60)}'::nummultirange, '{[2024-01-01,2024-01-02),[2024-01-02,2024-01-03)}'::datemultirange = '{[2024-01-01,2024-01-03)}'::datemultirange, '{[2024-01-01 00:00:00,2024-01-03 00:00:00)}'::tsmultirange @> TIMESTAMP '2024-01-02 12:00:00', '{[2024-01-01 00:00:00+00,2024-01-03 00:00:00+00)}'::tstzmultirange @> TIMESTAMPTZ '2024-01-02 12:00:00+00'";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result
            .expect("remaining built-in multirange families")
            .rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
        ]]
    );
}

#[test]
fn should_aggregate_builtin_ranges_into_a_multirange() {
    // Arrange
    // Until range types are admitted as table columns, table creation masks
    // this aggregate result assertion and records the prerequisite explicitly.
    let fixture = sql_fixture("issue-905-range-union-aggregate", &[]);
    fixture
        .execute("CREATE TABLE issue_905_range_union_input (value int4range)")
        .expect("create a range-valued aggregate input table");
    fixture
        .execute("INSERT INTO issue_905_range_union_input VALUES ('[1,5)'::int4range), ('[2,7)'::int4range), ('[3,4)'::int4range)")
        .expect("insert PostgreSQL range aggregate inputs");
    let sql = "SELECT range_agg(value)::text FROM issue_905_range_union_input";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("range aggregate").rows(),
        &[vec![Value::String("{[1,7)}".to_owned())]]
    );
}

#[test]
fn should_intersect_ranges_with_a_range_aggregate() {
    // Arrange
    // Until range types are admitted as table columns, table creation masks
    // this aggregate result assertion and records the prerequisite explicitly.
    let fixture = sql_fixture("issue-905-range-intersection-aggregate", &[]);
    fixture
        .execute("CREATE TABLE issue_905_range_intersection_input (value int4range)")
        .expect("create a range-valued intersection input table");
    fixture
        .execute("INSERT INTO issue_905_range_intersection_input VALUES ('[1,5)'::int4range), ('[2,7)'::int4range), ('[3,4)'::int4range)")
        .expect("insert PostgreSQL range intersection inputs");
    let sql = "SELECT range_intersect_agg(value)::text FROM issue_905_range_intersection_input";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("range-intersection aggregate").rows(),
        &[vec![Value::String("[3,4)".to_owned())]]
    );
}
