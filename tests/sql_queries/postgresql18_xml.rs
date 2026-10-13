//! PostgreSQL 18 XML, `XPath` and XMLTABLE behavior tracked by #906.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_evaluate_xpath_predicates_on_unicode_documents() {
    // Arrange
    let fixture = sql_fixture("issue-906-xml-xpath-unicode", &[]);
    let sql = "SELECT XMLSERIALIZE(DOCUMENT XMLPARSE(DOCUMENT '<root><item>é</item><item>two</item></root>') AS TEXT) = '<root><item>é</item><item>two</item></root>', xpath_exists('/root/item[text()=\"é\"]', XMLPARSE(DOCUMENT '<root><item>é</item><item>two</item></root>')), array_length(xpath('/root/item/text()', XMLPARSE(DOCUMENT '<root><item>é</item><item>two</item></root>')), 1) = 2";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("XML serialization and XPath").rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
        ]]
    );
}

#[test]
fn should_extract_typed_rows_from_xmltable_in_document_order() {
    // Arrange
    let fixture = sql_fixture("issue-906-xmltable-typed-rows", &[]);
    let sql = "SELECT id, label FROM XMLTABLE('/root/item' PASSING XMLPARSE(DOCUMENT '<root><item>é</item><item>two</item></root>') COLUMNS id FOR ORDINALITY, label TEXT PATH 'text()') ORDER BY id";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("XMLTABLE typed row extraction").rows(),
        &[
            vec![Value::Int64(1), Value::String("é".to_owned())],
            vec![Value::Int64(2), Value::String("two".to_owned())],
        ]
    );
}

#[test]
fn should_construct_xml_elements_with_attributes_unicode_content() {
    // Arrange
    let fixture = sql_fixture("issue-906-xmlelement-attributes", &[]);
    let sql =
        "SELECT XMLSERIALIZE(CONTENT XMLELEMENT(NAME item, XMLATTRIBUTES(7 AS id), 'é') AS TEXT)";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result
            .expect("XML element constructor and serialization")
            .rows(),
        &[vec![Value::String("<item id=\"7\">é</item>".to_owned())]]
    );
}

#[test]
fn should_extract_xmltable_rows_with_explicit_namespace_mapping() {
    // Arrange
    let fixture = sql_fixture("issue-906-xmltable-namespace-mapping", &[]);
    let sql = "SELECT label FROM XMLTABLE(XMLNAMESPACES('urn:example' AS x), '/x:root/x:item' PASSING XMLPARSE(DOCUMENT '<root xmlns=\"urn:example\"><item>A</item><item>B</item></root>') COLUMNS label TEXT PATH 'text()') ORDER BY label";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("XMLTABLE namespace mapping").rows(),
        &[
            vec![Value::String("A".to_owned())],
            vec![Value::String("B".to_owned())],
        ]
    );
}

#[test]
fn should_aggregate_xml_values_while_skipping_null_inputs() {
    // Arrange
    let fixture = sql_fixture("issue-906-xmlagg-null-input", &[]);
    let sql = "SELECT XMLSERIALIZE(CONTENT XMLAGG(value) AS TEXT) FROM (VALUES (XMLELEMENT(NAME item, 'one')), (NULL::xml)) AS input(value)";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("XMLAGG skips NULL inputs").rows(),
        &[vec![Value::String("<item>one</item>".to_owned())]]
    );
}
