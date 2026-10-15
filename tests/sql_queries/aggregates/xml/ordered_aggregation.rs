use super::*;

#[test]
fn should_order_xmlagg_elements_by_aggregate_order() {
    // Arrange
    let fixture = sql_fixture("issue-906-xmlagg-ordered-inputs", &[]);
    let sql = "SELECT XMLSERIALIZE(CONTENT XMLAGG(value ORDER BY ordinal DESC) AS TEXT) \
               FROM (VALUES \
                 (1, XMLELEMENT(NAME item, 'one')), \
                 (3, XMLELEMENT(NAME item, 'three')), \
                 (2, XMLELEMENT(NAME item, 'two')), \
                 (4, NULL::xml)) AS input(ordinal, value)";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("ordered XML aggregate").rows(),
        &[vec![Value::String(
            "<item>three</item><item>two</item><item>one</item>".to_owned()
        )]]
    );
}
