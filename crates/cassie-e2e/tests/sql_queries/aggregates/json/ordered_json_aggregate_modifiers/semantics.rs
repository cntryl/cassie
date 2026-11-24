//! DISTINCT, FILTER, and aggregate-local ORDER BY interaction for JSON collections.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_preserve_distinct_filter_order_for_json_aggregates() {
    // Arrange
    let fixture = sql_fixture("issue-811-json-aggregate-modifiers", &[]);

    // Act
    let result = fixture.execute(
        "SELECT json_agg(DISTINCT payload ORDER BY payload) \
                    FILTER (WHERE selected)::TEXT, \
                pg_typeof(json_agg(DISTINCT payload ORDER BY payload) \
                    FILTER (WHERE selected))::OID, \
                jsonb_agg(DISTINCT payload ORDER BY payload) \
                    FILTER (WHERE selected)::TEXT, \
                pg_typeof(jsonb_agg(DISTINCT payload ORDER BY payload) \
                    FILTER (WHERE selected))::OID \
         FROM (VALUES (2,TRUE),(1,TRUE),(2,FALSE),(NULL::INT,TRUE), \
                      (NULL::INT,TRUE),(0,FALSE)) AS inputs(payload,selected)",
    );

    // Assert
    let result = result.expect("JSON collection aggregates combine DISTINCT, FILTER, and order");
    assert_eq!(
        result.rows(),
        &[vec![
            Value::String("[1, 2, null]".to_owned()),
            Value::Int64(114),
            Value::String("[1, 2, null]".to_owned()),
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
