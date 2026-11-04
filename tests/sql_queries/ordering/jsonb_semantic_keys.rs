//! Exact JSONB semantic keys across relational operators (#809).
use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_order_jsonb_values_by_their_native_type_order() {
    // Arrange
    let fixture = sql_fixture("jsonb-semantic-type-order", &[]);
    // Act
    let result = fixture.execute(
        r#"SELECT id FROM (VALUES
        (1,'null'::JSONB),(2,'[]'::JSONB),(3,'"x"'::JSONB),(4,'10'::JSONB),
        (5,'2.0'::JSONB),(6,'2'::JSONB),(7,'false'::JSONB),(8,'{}'::JSONB),
        (9,'[1]'::JSONB),(10,'[0,0]'::JSONB),(11,'{"b":1}'::JSONB),
        (12,'{"aa":0}'::JSONB)) AS docs(id,payload) ORDER BY payload,id"#,
    );
    // Assert
    assert_eq!(
        result.expect("JSONB native order").rows(),
        &[2, 1, 3, 5, 6, 4, 7, 9, 10, 8, 12, 11].map(|id| vec![Value::Int64(id)])
    );
}

#[test]
fn should_group_jsonb_numeric_equivalents_without_merging_sql_null() {
    // Arrange
    let fixture = sql_fixture("jsonb-semantic-group-identity", &[]);
    // Act
    let result = fixture.execute(
        r#"SELECT count(*) FROM (VALUES
        ('2'::JSONB),('2.0'::JSONB),('2e0'::JSONB),('-0'::JSONB),('0.0'::JSONB),
        ('{"n":1.0}'::JSONB),('{"n":1}'::JSONB),(NULL::JSONB),('null'::JSONB))
        AS docs(payload) GROUP BY payload ORDER BY count(*),payload"#,
    );
    // Assert
    assert_eq!(
        result.expect("JSONB exact groups").rows(),
        &[1, 1, 2, 2, 3].map(|count| vec![Value::Int64(count)])
    );
}

#[test]
fn should_deduplicate_jsonb_with_exact_decimal_identity() {
    // Arrange
    let fixture = sql_fixture("jsonb-semantic-distinct", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM (SELECT DISTINCT payload FROM         (VALUES ('9007199254740993.0000000000000001'::JSONB),                 ('9007199254740993.00000000000000010'::JSONB),                 ('9007199254740993'::JSONB)) AS docs(payload)) AS unique_docs");
    // Assert
    assert_eq!(
        result.expect("exact JSONB distinct").rows(),
        &[vec![Value::Int64(2)]]
    );
}

#[test]
fn should_use_jsonb_identity_for_set_operation_multiplicity() {
    // Arrange
    let fixture = sql_fixture("jsonb-semantic-sets", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM         (SELECT '1.0'::JSONB AS payload UNION SELECT '1e0'::JSONB AS payload) AS docs");
    // Assert
    assert_eq!(
        result.expect("JSONB UNION identity").rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_partition_windows_by_jsonb_numeric_identity() {
    // Arrange
    let fixture = sql_fixture("jsonb-semantic-window-partition", &[]);
    // Act
    let result = fixture.execute("SELECT id,count(*) OVER (PARTITION BY payload) AS n         FROM (VALUES (1,'1'::JSONB),(2,'1.00'::JSONB),(3,'2'::JSONB))         AS docs(id,payload) ORDER BY id");
    // Assert
    assert_eq!(
        result.expect("JSONB window partition").rows(),
        &[
            vec![Value::Int64(1), Value::Int64(2)],
            vec![Value::Int64(2), Value::Int64(2)],
            vec![Value::Int64(3), Value::Int64(1)]
        ]
    );
}

#[test]
fn should_preserve_jsonb_identity_through_computed_projections() {
    // Arrange
    let fixture = sql_fixture("jsonb-semantic-derived-projection", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM (SELECT DISTINCT         jsonb_build_object('n',n) AS payload FROM (VALUES (1.0::NUMERIC),         (1.00::NUMERIC)) AS numbers(n)) AS docs");
    // Assert
    assert_eq!(
        result.expect("computed JSONB metadata").rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_preserve_text_identity_when_values_look_like_jsonb() {
    // Arrange
    let fixture = sql_fixture("jsonb-semantic-text-contrast", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM         (SELECT DISTINCT payload FROM (VALUES ('1.0'::TEXT),('1'::TEXT)) AS docs(payload))         AS unique_docs");
    // Assert
    assert_eq!(
        result.expect("SQL TEXT remains textual").rows(),
        &[vec![Value::Int64(2)]]
    );
}

#[test]
fn should_preserve_jsonb_array_element_identity() {
    // Arrange
    let fixture = sql_fixture("jsonb-semantic-native-arrays", &[]);
    // Act
    let result = fixture.execute("SELECT count(*) FROM (SELECT DISTINCT payload FROM         (VALUES (ARRAY['1'::JSONB,'null'::JSONB,NULL::JSONB]),                 (ARRAY['1.00'::JSONB,'null'::JSONB,NULL::JSONB]),                 (ARRAY['1'::JSONB,NULL::JSONB,'null'::JSONB])) AS docs(payload)) AS unique_docs");
    // Assert
    assert_eq!(
        result.expect("JSONB[] element identity").rows(),
        &[vec![Value::Int64(2)]]
    );
}

#[test]
fn should_deduplicate_recursive_union_using_declared_jsonb_fields() {
    // Arrange
    let fixture = sql_fixture("jsonb-semantic-recursive-union", &[]);
    // Act
    let result = fixture.execute(
        "WITH RECURSIVE docs(payload) AS \
        (SELECT '1.0'::JSONB UNION SELECT '1.00'::JSONB FROM docs) SELECT count(*) FROM docs",
    );
    // Assert
    assert_eq!(
        result.expect("recursive JSONB identity").rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_use_jsonb_identity_for_aggregate_distinct() {
    // Arrange
    let fixture = sql_fixture("jsonb-semantic-aggregate-distinct", &[]);
    // Act
    let result = fixture.execute(
        "SELECT count(DISTINCT payload) FROM \
        (VALUES ('1'::JSONB),('1.0'::JSONB),('2'::JSONB)) AS docs(payload)",
    );
    // Assert
    assert_eq!(
        result.expect("aggregate JSONB identity").rows(),
        &[vec![Value::Int64(2)]]
    );
}

#[test]
fn should_compare_jsonb_join_targets_on_the_qualified_scalar_path() {
    // Arrange
    let fixture = sql_fixture("jsonb-semantic-qualified-join", &[]);
    // Act
    let result = fixture.execute(
        "SELECT count(*) FROM (VALUES ('1.0'::JSONB),('2'::JSONB)) \
        AS lhs(payload) JOIN (VALUES ('1e0'::JSONB)) AS rhs(payload) ON lhs.payload=rhs.payload",
    );
    // Assert
    assert_eq!(
        result.expect("qualified JSONB scalar equality").rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_preserve_stored_jsonb_identity_across_native_scan_rows() {
    // Arrange
    let fixture = sql_fixture(
        "jsonb-semantic-stored-rows",
        &[
            "CREATE TABLE docs (id INT PRIMARY KEY, payload JSONB)",
            "INSERT INTO docs VALUES (1,'1.0'::JSONB),(2,'1.00'::JSONB),(3,'2'::JSONB)",
        ],
    );
    // Act
    let result =
        fixture.execute("SELECT count(*) FROM (SELECT DISTINCT payload FROM docs) AS unique_docs");
    // Assert
    assert_eq!(
        result.expect("stored JSONB metadata").rows(),
        &[vec![Value::Int64(2)]]
    );
}

#[test]
fn should_preserve_jsonb_identity_after_collection_aggregate_projection() {
    // Arrange
    let fixture = sql_fixture("jsonb-semantic-aggregate-output", &[]);
    // Act
    let result = fixture.execute(
        "SELECT count(*) FROM (SELECT DISTINCT payload FROM \
        (SELECT jsonb_agg(n) AS payload FROM (VALUES (1,1.0::NUMERIC),(2,1.00::NUMERIC)) \
        AS numbers(g,n) GROUP BY g) AS grouped_docs) AS unique_docs",
    );
    // Assert
    assert_eq!(
        result.expect("aggregate JSONB output metadata").rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_preserve_jsonb_identity_after_value_window_projection() {
    // Arrange
    let fixture = sql_fixture("jsonb-semantic-window-output", &[]);
    // Act
    let result = fixture.execute(
        "SELECT count(*) FROM (SELECT DISTINCT \
        first_value(payload) OVER (PARTITION BY id ORDER BY id) AS payload \
        FROM (VALUES (1,'1.0'::JSONB),(2,'1.00'::JSONB)) AS docs(id,payload)) AS unique_docs",
    );
    // Assert
    assert_eq!(
        result.expect("window JSONB output metadata").rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_preserve_jsonb_identity_from_a_lateral_outer_column() {
    // Arrange
    let fixture = sql_fixture("jsonb-semantic-lateral-outer-column", &[]);
    // Act
    let result = fixture.execute(
        "SELECT count(*) FROM (SELECT DISTINCT nested.payload \
        FROM (VALUES ('1.0'::JSONB),('1.00'::JSONB)) AS outer_docs(payload), \
        LATERAL (SELECT outer_docs.payload AS payload) AS nested) AS unique_docs",
    );
    // Assert
    assert_eq!(
        result.expect("outer-column JSONB metadata").rows(),
        &[vec![Value::Int64(1)]]
    );
}
