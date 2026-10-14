//! PostgreSQL 18 range and multirange value behavior tracked by #905.

use cassie::app::Cassie;
use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

struct RangeDirectory(String);

impl Drop for RangeDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

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
    let sql = "SELECT lower_inf(daterange(NULL, NULL, '()')) AND upper_inf(daterange(NULL, NULL, '()')), int4range(1, 5, '[)') @> 1, int4range(1, 5, '[)') @> 4, NOT (int4range(1, 5, '[)') @> 5), isempty('empty'::int8range), isempty(int8range(1, 1, '[)'))";

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
fn should_construct_a_multirange_from_one_range() {
    // Arrange
    let fixture = sql_fixture("issue-905-multirange-constructor", &[]);
    let sql = "SELECT multirange(int4range(1, 3, '[)'))::TEXT";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("multirange constructor").rows(),
        &[vec![Value::String("{[1,3)}".to_owned())]]
    );
}

#[test]
fn should_unnest_multirange_segments_in_order() {
    // Arrange
    let fixture = sql_fixture("issue-905-multirange-unnest", &[]);
    let sql = "SELECT unnest(multirange(int4range(1, 3, '[)')) + multirange(int4range(5, 7, '[)')))::TEXT";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("multirange UNNEST segments").rows(),
        &[
            vec![Value::String("[1,3)".to_owned())],
            vec![Value::String("[5,7)".to_owned())],
        ]
    );
}

#[test]
fn should_apply_multirange_containment_overlap() {
    // Arrange
    let fixture = sql_fixture("issue-905-multirange-relations", &[]);
    let sql = "SELECT '{[1,3),[5,7)}'::int4multirange @> 2, '{[1,3),[5,7)}'::int4multirange @> '[5,7)'::int4range, '{[1,3),[5,7)}'::int4multirange && '{[6,8)}'::int4multirange, NOT ('{[1,3),[5,7)}'::int4multirange && '{[3,5)}'::int4multirange)";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("multirange containment and overlap").rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
        ]]
    );
}

#[test]
fn should_inspect_multirange_outer_bounds() {
    // Arrange
    let fixture = sql_fixture("issue-905-multirange-bounds", &[]);
    let sql = "SELECT lower(multirange(int4range(2, 5, '[)')) + multirange(int4range(10, 12, '[)'))), upper(multirange(int4range(2, 5, '[)')) + multirange(int4range(10, 12, '[)')))";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("multirange outer bounds").rows(),
        &[vec![Value::Int64(2), Value::Int64(12)]]
    );
}

#[test]
fn should_inspect_finite_discrete_continuous_range_bounds() {
    // Arrange
    let fixture = sql_fixture("issue-905-finite-range-bounds", &[]);
    let sql = "SELECT lower(int4range(2, 7, '[)')) = 2, upper(int4range(2, 7, '[]')) = 8, lower_inc(int4range(2, 7, '[]')) AND NOT upper_inc(int4range(2, 7, '[]')), lower(int4range(2, 7, '(]')) = 3, lower_inc(numrange(2, 7, '[]')) AND upper_inc(numrange(2, 7, '[]'))";

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
    let fixture = sql_fixture("issue-905-range-union-aggregate", &[]);
    fixture
        .execute("CREATE TABLE issue_905_range_aggregate_rows (span INT4RANGE)")
        .expect("create a table with range values");
    fixture
        .execute(
            "INSERT INTO issue_905_range_aggregate_rows VALUES ('[1,5)'::INT4RANGE), ('[4,7)'::INT4RANGE), ('[9,11)'::INT4RANGE), (NULL)",
        )
        .expect("insert overlapping, separated and NULL ranges");

    // Act
    let result =
        fixture.execute("SELECT range_agg(span)::TEXT FROM issue_905_range_aggregate_rows");

    // Assert
    assert_eq!(
        result
            .expect("range aggregate across overlapping and separated inputs")
            .rows(),
        &[vec![Value::String("{[1,7),[9,11)}".to_owned())]]
    );
}

#[test]
fn should_intersect_ranges_with_a_range_aggregate() {
    // Arrange
    let fixture = sql_fixture("issue-905-range-intersection-aggregate", &[]);
    fixture
        .execute("CREATE TABLE issue_905_range_intersection_rows (span INT4RANGE)")
        .expect("create a table with range values");
    fixture
        .execute(
            "INSERT INTO issue_905_range_intersection_rows VALUES ('[1,5)'::INT4RANGE), ('[3,7)'::INT4RANGE), ('[4,6)'::INT4RANGE), (NULL)",
        )
        .expect("insert intersecting and NULL ranges");

    // Act
    let result = fixture
        .execute("SELECT range_intersect_agg(span)::TEXT FROM issue_905_range_intersection_rows");

    // Assert
    assert_eq!(
        result
            .expect("range intersection across multiple inputs")
            .rows(),
        &[vec![Value::String("[4,5)".to_owned())]]
    );
}

#[test]
fn should_match_range_predicates_in_join_rows() {
    // Arrange
    let fixture = sql_fixture("issue-905-range-table-predicates", &[]);
    fixture
        .execute("CREATE TABLE issue_905_range_rows (id BIGINT PRIMARY KEY, span INT4RANGE)")
        .expect("create a table with a range column");
    fixture
        .execute(
            "INSERT INTO issue_905_range_rows VALUES (1, '[1,3)'::INT4RANGE), (2, '[5,7)'::INT4RANGE), (3, '[2,5)'::INT4RANGE)",
        )
        .expect("insert ranges for table predicates and joins");

    // Act
    let result = fixture.execute(
        "SELECT left_row.id, right_row.id FROM issue_905_range_rows AS left_row JOIN issue_905_range_rows AS right_row ON left_row.span && right_row.span WHERE left_row.span @> 2 ORDER BY left_row.id, right_row.id",
    );

    // Assert
    assert_eq!(
        result.expect("table range predicates and joins").rows(),
        &[
            vec![Value::Int64(1), Value::Int64(1)],
            vec![Value::Int64(1), Value::Int64(3)],
            vec![Value::Int64(3), Value::Int64(1)],
            vec![Value::Int64(3), Value::Int64(3)],
        ]
    );
}

#[test]
fn should_keep_range_text_values_across_restart() {
    // Arrange
    crate::support_sql::use_local_storage();
    let path = crate::support_sql::data_dir("issue-905-range-restart");
    let _directory = RangeDirectory(path.clone());
    {
        let cassie = Cassie::new_with_data_dir(&path).expect("create range fixture");
        cassie.startup().expect("start range fixture");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE issue_905_restart_ranges (value INT4RANGE)",
                vec![],
            )
            .expect("create a range column");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO issue_905_restart_ranges VALUES (int4range(1, 5, '[)'))",
                vec![],
            )
            .expect("store a range value");
    }
    let restarted = Cassie::new_with_data_dir(&path).expect("reopen range fixture");
    restarted.startup().expect("restart range fixture");
    let session = restarted.create_session("tester", None);

    // Act
    let result = restarted.execute_sql(
        &session,
        "SELECT value::TEXT FROM issue_905_restart_ranges",
        vec![],
    );

    // Assert
    assert_eq!(
        result.expect("range value survives restart").rows(),
        &[vec![Value::String("[1,5)".to_owned())]]
    );
    drop(session);
    restarted.shutdown();
    drop(restarted);
}

#[test]
fn should_render_range_constructor_bounds_in_postgresql_form() {
    // Arrange
    let fixture = sql_fixture("issue-905-range-family-canonical-bounds", &[]);
    let sql = "SELECT int8range(1, 3, '[]')::TEXT, daterange(DATE '2024-02-28', DATE '2024-03-01', '[]')::TEXT, numrange(1.20, 2.40, '[]')::TEXT, tsrange(TIMESTAMP '2024-01-01 00:00:00', TIMESTAMP '2024-01-02 00:00:00', '[]')::TEXT, multirange(int8range(1, 3, '[]'))::TEXT, multirange(daterange(DATE '2024-02-28', DATE '2024-03-01', '[]'))::TEXT, multirange(numrange(1.20, 2.40, '[]'))::TEXT, multirange(tsrange(TIMESTAMP '2024-01-01 00:00:00', TIMESTAMP '2024-01-02 00:00:00', '[]'))::TEXT";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("built-in range family bound forms").rows(),
        &[vec![
            Value::String("[1,4)".to_owned()),
            Value::String("[2024-02-28,2024-03-02)".to_owned()),
            Value::String("[1.20,2.40]".to_owned()),
            Value::String("[\"2024-01-01 00:00:00\",\"2024-01-02 00:00:00\"]".to_owned()),
            Value::String("{[1,4)}".to_owned()),
            Value::String("{[2024-02-28,2024-03-02)}".to_owned()),
            Value::String("{[1.20,2.40]}".to_owned()),
            Value::String("{[\"2024-01-01 00:00:00\",\"2024-01-02 00:00:00\"]}".to_owned()),
        ]]
    );
}

#[test]
fn should_inspect_range_boundary_flags() {
    // Arrange
    let fixture = sql_fixture("issue-905-range-empty-infinity-inspection", &[]);
    let sql = "SELECT isempty('empty'::INT8RANGE), lower('empty'::INT8RANGE) IS NULL, upper('empty'::INT8RANGE) IS NULL, lower_inf('empty'::INT8RANGE), upper_inf('empty'::INT8RANGE), lower_inf(int8range(NULL, 5, '()')), upper_inf(int8range(NULL, 5, '()')), lower_inc(int8range(NULL, 5, '()')), upper_inc(int8range(NULL, 5, '()')), lower_inf(numrange(NULL, 3, '()')), upper_inf(numrange(NULL, 3, '()')), lower_inc(numrange(NULL, 3, '()')), upper_inc(numrange(NULL, 3, '()'))";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result
            .expect("empty and unbounded range inspections")
            .rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(false),
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(false),
            Value::Bool(false),
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(false),
            Value::Bool(false),
        ]]
    );
}

#[test]
fn should_inspect_empty_multirange_bound_flags() {
    // Arrange
    let fixture = sql_fixture("issue-905-empty-multirange-inspection", &[]);
    let sql = "SELECT isempty('{}'::INT4MULTIRANGE), lower('{}'::INT4MULTIRANGE) IS NULL, upper('{}'::INT4MULTIRANGE) IS NULL, lower_inf('{}'::INT4MULTIRANGE), upper_inf('{}'::INT4MULTIRANGE)";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("empty multirange bound flags").rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(false),
        ]]
    );
}

#[test]
fn should_apply_directional_operators_at_exclusive_range_boundaries() {
    // Arrange
    let fixture = sql_fixture("issue-905-range-directional-operators", &[]);
    let sql = "SELECT int4range(1, 3, '[)') << int4range(3, 5, '[)'), int4range(3, 5, '[)') >> int4range(1, 3, '[)'), int4range(1, 3, '[)') &< int4range(3, 5, '[)'), int4range(3, 5, '[)') &> int4range(1, 3, '[)'), int4range(1, 3, '[)') -|- int4range(3, 5, '[)'), int4range(1, 3, '[)') && int4range(3, 5, '[)'), int4range(1, 3, '[)') <@ int4range(0, 4, '[)'), int4range(1, 3, '[)') @> int4range(0, 4, '[)'), 2 <@ int4range(1, 3, '[)'), 3 <@ int4range(1, 3, '[)')";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result
            .expect("directional range operator boundary matrix")
            .rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(true),
            Value::Bool(false),
        ]]
    );
}

#[test]
fn should_order_empty_ranges_before_finite_ranges() {
    // Arrange
    let fixture = sql_fixture("issue-905-range-empty-order", &[]);
    let sql = "SELECT 'empty'::INT4RANGE = int4range(1, 1, '[)'), 'empty'::INT4RANGE < int4range(1, 3, '[)'), int4range(1, 3, '[)') > 'empty'::INT4RANGE, int4range(1, 3, '[)') < int4range(1, 4, '[)')";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result
            .expect("range btree ordering around empty values")
            .rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
        ]]
    );
}

#[test]
fn should_preserve_empty_multirange_operator_identities() {
    // Arrange
    let fixture = sql_fixture("issue-905-empty-multirange-relations", &[]);
    let sql = "SELECT '{}'::INT4MULTIRANGE @> 'empty'::INT4RANGE, 'empty'::INT4RANGE <@ '{[1,3)}'::INT4MULTIRANGE, '{}'::INT4MULTIRANGE <@ '{[1,3)}'::INT4MULTIRANGE, '{}'::INT4MULTIRANGE && '{[1,3)}'::INT4MULTIRANGE, ('{}'::INT4MULTIRANGE + '{[1,2),[4,5)}'::INT4MULTIRANGE)::TEXT, ('{}'::INT4MULTIRANGE * '{[1,2)}'::INT4MULTIRANGE)::TEXT";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("empty multirange operations").rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(false),
            Value::String("{[1,2),[4,5)}".to_owned()),
            Value::String("{}".to_owned()),
        ]]
    );
}

#[test]
fn should_preserve_range_aggregate_empty_nulls() {
    // Arrange
    let fixture = sql_fixture("issue-905-range-aggregate-empty-null", &[]);
    fixture
        .execute("CREATE TABLE issue_905_range_aggregate_edges (kind TEXT, span INT4RANGE)")
        .expect("create range aggregate edge table");
    fixture
        .execute("INSERT INTO issue_905_range_aggregate_edges VALUES ('empty_only', 'empty'::INT4RANGE), ('mixed', 'empty'::INT4RANGE), ('mixed', '[1,3)'::INT4RANGE), ('null_only', NULL)")
        .expect("insert empty, finite, and null ranges");

    // Act
    let result = fixture.execute(
        "SELECT kind, range_agg(span)::TEXT, range_intersect_agg(span)::TEXT FROM issue_905_range_aggregate_edges GROUP BY kind ORDER BY kind",
    );

    // Assert
    assert_eq!(
        result
            .expect("range aggregates preserve empty and null identities")
            .rows(),
        &[
            vec![
                Value::String("empty_only".to_owned()),
                Value::String("{}".to_owned()),
                Value::String("empty".to_owned()),
            ],
            vec![
                Value::String("mixed".to_owned()),
                Value::String("{[1,3)}".to_owned()),
                Value::String("empty".to_owned()),
            ],
            vec![
                Value::String("null_only".to_owned()),
                Value::Null,
                Value::Null
            ],
        ]
    );
}
