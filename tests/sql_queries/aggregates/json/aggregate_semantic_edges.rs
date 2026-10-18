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
