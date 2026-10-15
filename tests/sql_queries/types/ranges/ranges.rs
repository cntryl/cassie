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
fn should_use_numeric_subtype_operators_for_custom_ranges() {
    // Arrange
    let fixture = sql_fixture("issue-905-custom-numeric-range", &[]);

    // Act
    fixture
        .execute("CREATE TYPE issue_905_numeric_range AS RANGE (subtype = NUMERIC)")
        .expect("create a custom range over numeric");
    let result = fixture.execute(
        "SELECT '[1.20,3.40]'::issue_905_numeric_range @> 2.5::NUMERIC, \
                '[1.20,3.40]'::issue_905_numeric_range @> 3.40::NUMERIC, \
                '[1.20,3.40]'::issue_905_numeric_range = '[1.2,3.4]'::issue_905_numeric_range",
    );

    // Assert
    assert_eq!(
        result
            .expect("custom numeric range subtype comparisons")
            .rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true)
        ]]
    );
}

#[test]
fn should_bind_custom_range_subtype_difference_to_its_routine() {
    // Arrange
    let fixture = sql_fixture("issue-905-custom-range-subtype-difference", &[]);

    // Act
    fixture
        .execute(
            "CREATE TYPE issue_905_float_range AS RANGE \
             (subtype = FLOAT8, subtype_diff = pg_catalog.float8mi)",
        )
        .expect("create a custom range with PostgreSQL's float8 subtype-difference routine");
    let value = fixture
        .execute(
            "SELECT '[1.0,3.0]'::issue_905_float_range @> 2.5::FLOAT8, \
                    '[1.0,3.0]'::issue_905_float_range::TEXT",
        )
        .expect("construct and compare values of the custom float8 range");
    let routine = fixture
        .execute(
            "SELECT range_type.typname, procedure.proname, \
                    procedure.oid::REGPROCEDURE::TEXT \
             FROM pg_catalog.pg_range AS range \
             JOIN pg_catalog.pg_type AS range_type \
               ON range_type.oid = range.rngtypid \
             JOIN pg_catalog.pg_proc AS procedure \
               ON procedure.oid = range.rngsubdiff \
             WHERE range_type.typname = 'issue_905_float_range'",
        )
        .expect("inspect the range's PostgreSQL subtype-difference identity");

    // Assert
    assert_eq!(
        value.rows(),
        &[vec![Value::Bool(true), Value::String("[1,3]".to_owned()),]]
    );
    assert_eq!(
        routine.rows(),
        &[vec![
            Value::String("issue_905_float_range".to_owned()),
            Value::String("float8mi".to_owned()),
            Value::String("float8mi(double precision, double precision)".to_owned()),
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
fn should_restore_builtin_range_families_after_restart() {
    // Arrange
    crate::support_sql::use_local_storage();
    let path = crate::support_sql::data_dir("issue-905-range-family-restart");
    let _directory = RangeDirectory(path.clone());
    {
        let cassie = Cassie::new_with_data_dir(&path).expect("create range-family fixture");
        cassie.startup().expect("start range-family fixture");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE issue_905_restart_range_families (\
                    int4_range INT4RANGE, int8_range INT8RANGE, numeric_range NUMRANGE, \
                    date_range DATERANGE, timestamp_range TSRANGE, timestamptz_range TSTZRANGE, \
                    int4_multirange INT4MULTIRANGE, int8_multirange INT8MULTIRANGE, \
                    numeric_multirange NUMMULTIRANGE, date_multirange DATEMULTIRANGE, \
                    timestamp_multirange TSMULTIRANGE, timestamptz_multirange TSTZMULTIRANGE)",
                vec![],
            )
            .expect("create columns for every built-in range family");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO issue_905_restart_range_families VALUES (\
                    '[1,5)'::INT4RANGE, '[1,5)'::INT8RANGE, '[1.20,2.40)'::NUMRANGE, \
                    '[2024-01-01,2024-01-03)'::DATERANGE, \
                    '[2024-01-01 00:00:00,2024-01-03 00:00:00)'::TSRANGE, \
                    '[2024-01-01 00:00:00+00,2024-01-03 00:00:00+00)'::TSTZRANGE, \
                    '{[1,3),[4,5)}'::INT4MULTIRANGE, '{[1,3),[4,5)}'::INT8MULTIRANGE, \
                    '{[1.20,2.40),[3.00,4.00)}'::NUMMULTIRANGE, \
                    '{[2024-01-01,2024-01-02),[2024-01-03,2024-01-04)}'::DATEMULTIRANGE, \
                    '{[2024-01-01 00:00:00,2024-01-02 00:00:00),\
                      [2024-01-03 00:00:00,2024-01-04 00:00:00)}'::TSMULTIRANGE, \
                    '{[2024-01-01 00:00:00+00,2024-01-02 00:00:00+00),\
                      [2024-01-03 00:00:00+00,2024-01-04 00:00:00+00)}'::TSTZMULTIRANGE)",
                vec![],
            )
            .expect("store every built-in range and multirange family");
    }
    let restarted = Cassie::new_with_data_dir(&path).expect("reopen range-family fixture");
    restarted.startup().expect("restart range-family fixture");
    let session = restarted.create_session("tester", None);

    // Act
    let result = restarted.execute_sql(
        &session,
        "SELECT int4_range = '[1,5)'::INT4RANGE, int8_range = '[1,5)'::INT8RANGE, \
                numeric_range = '[1.20,2.40)'::NUMRANGE, \
                date_range = '[2024-01-01,2024-01-03)'::DATERANGE, \
                timestamp_range = '[2024-01-01 00:00:00,2024-01-03 00:00:00)'::TSRANGE, \
                timestamptz_range = '[2024-01-01 00:00:00+00,2024-01-03 00:00:00+00)'::TSTZRANGE, \
                int4_multirange = '{[1,3),[4,5)}'::INT4MULTIRANGE, \
                int8_multirange = '{[1,3),[4,5)}'::INT8MULTIRANGE, \
                numeric_multirange = '{[1.20,2.40),[3.00,4.00)}'::NUMMULTIRANGE, \
                date_multirange = '{[2024-01-01,2024-01-02),[2024-01-03,2024-01-04)}'::DATEMULTIRANGE, \
                timestamp_multirange = \
                    '{[2024-01-01 00:00:00,2024-01-02 00:00:00),\
                      [2024-01-03 00:00:00,2024-01-04 00:00:00)}'::TSMULTIRANGE, \
                timestamptz_multirange = \
                    '{[2024-01-01 00:00:00+00,2024-01-02 00:00:00+00),\
                      [2024-01-03 00:00:00+00,2024-01-04 00:00:00+00)}'::TSTZMULTIRANGE \
         FROM issue_905_restart_range_families",
        vec![],
    );

    // Assert
    assert_eq!(
        result
            .expect("every built-in range family survives restart")
            .rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
        ]]
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
fn should_preserve_continuous_multirange_gaps_at_excluded_touchpoints() {
    // Arrange
    let fixture = sql_fixture("issue-905-continuous-multirange-touchpoints", &[]);
    let sql = "SELECT '{(1,2),(2,3)}'::NUMMULTIRANGE::TEXT, '{[1,2),[2,3]}'::NUMMULTIRANGE::TEXT, '{[1,2),[2,3)}'::NUMMULTIRANGE::TEXT, '{[1,2],[2,3]}'::NUMMULTIRANGE::TEXT, '{(1,2),(2,3)}'::NUMMULTIRANGE @> 2::NUMERIC, '{[1,2),[2,3]}'::NUMMULTIRANGE @> 2::NUMERIC";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result
            .expect("continuous multiranges preserve or merge touching bounds")
            .rows(),
        &[vec![
            Value::String("{(1,2),(2,3)}".to_owned()),
            Value::String("{[1,3]}".to_owned()),
            Value::String("{[1,3)}".to_owned()),
            Value::String("{[1,3]}".to_owned()),
            Value::Bool(false),
            Value::Bool(true),
        ]]
    );
}

#[test]
fn should_preserve_unbounded_multirange_boundary_semantics() {
    // Arrange
    let fixture = sql_fixture("issue-905-unbounded-multirange-boundaries", &[]);
    let sql = "SELECT lower('{(,3),[7,)}'::INT4MULTIRANGE) IS NULL, upper('{(,3),[7,)}'::INT4MULTIRANGE) IS NULL, lower_inf('{(,3),[7,)}'::INT4MULTIRANGE), upper_inf('{(,3),[7,)}'::INT4MULTIRANGE), lower_inc('{(,3),[7,)}'::INT4MULTIRANGE), upper_inc('{(,3),[7,)}'::INT4MULTIRANGE), '{(,3),[7,)}'::INT4MULTIRANGE @> -100, '{(,3),[7,)}'::INT4MULTIRANGE @> 6, '{(,3),[7,)}'::INT4MULTIRANGE @> 100, '{(,3),[7,)}'::INT4MULTIRANGE::TEXT";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result
            .expect("unbounded multirange bounds and scalar membership")
            .rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(false),
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(true),
            Value::String("{(,3),[7,)}".to_owned()),
        ]]
    );
}

#[test]
fn should_compare_multirange_order_with_mixed_type_adjacency() {
    // Arrange
    let fixture = sql_fixture("issue-905-multirange-order-and-adjacency", &[]);
    let sql = "SELECT '{[1,3)}'::INT4MULTIRANGE -|- '{[3,5)}'::INT4MULTIRANGE, '{[1,3)}'::INT4MULTIRANGE -|- '{[4,6)}'::INT4MULTIRANGE, '{[1,3)}'::INT4MULTIRANGE -|- '[3,5)'::INT4RANGE, '[3,5)'::INT4RANGE -|- '{[1,3)}'::INT4MULTIRANGE, '{[1,3)}'::INT4MULTIRANGE < '{[1,4)}'::INT4MULTIRANGE, '{[1,4)}'::INT4MULTIRANGE > '{[1,3)}'::INT4MULTIRANGE";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result
            .expect("multirange ordering and mixed range adjacency")
            .rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
        ]]
    );
}
