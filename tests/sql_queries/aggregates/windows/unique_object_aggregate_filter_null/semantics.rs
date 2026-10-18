#[test]
fn should_filter_excluded_duplicates_before_ordered_unique_strict_json_aggregation() {
    // Arrange
    let fixture = sql_fixture(
        "issue-811-filtered-unique-strict-json-objects",
        &[
            "CREATE TABLE issue_811_filtered_unique_json_values (position INT, key TEXT, value INT, included BOOLEAN)",
            "INSERT INTO issue_811_filtered_unique_json_values VALUES (1,'z',1,TRUE),(2,'a',2,FALSE),(3,'a',3,TRUE),(4,'z',9,FALSE),(5,'omit',NULL,TRUE)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT json_object_agg_unique_strict(key,value ORDER BY position) \
                    FILTER (WHERE included)::TEXT, \
                pg_typeof(json_object_agg_unique_strict(key,value ORDER BY position) \
                    FILTER (WHERE included))::OID, \
                jsonb_object_agg_unique_strict(key,value ORDER BY position) \
                    FILTER (WHERE included)::TEXT, \
                pg_typeof(jsonb_object_agg_unique_strict(key,value ORDER BY position) \
                    FILTER (WHERE included))::OID \
         FROM issue_811_filtered_unique_json_values",
    );

    // Assert
    let result = result.expect("FILTER excludes duplicate keys before unique validation");
    assert_eq!(
        result.rows(),
        &[vec![
            Value::String("{ \"z\" : 1, \"a\" : 3 }".to_owned()),
            Value::Int64(114),
            Value::String("{\"a\": 3, \"z\": 1}".to_owned()),
            Value::Int64(3802),
        ]]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 26, 25, 26]
    );
}
