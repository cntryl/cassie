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

#[test]
fn should_report_well_formed_xml_documents() {
    // Arrange
    let fixture = sql_fixture("issue-906-xml-well-formedness", &[]);
    let sql = "SELECT xml_is_well_formed_document('<root/>'), xml_is_well_formed_document('<root><item></root>')";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("XML well-formedness predicates").rows(),
        &[vec![Value::Bool(true), Value::Bool(false)]]
    );
}

#[test]
fn should_report_well_formed_xml_content() {
    // Arrange
    let fixture = sql_fixture("issue-906-xml-content-well-formedness", &[]);
    let sql =
        "SELECT xml_is_well_formed_content('<a/><b/>'), xml_is_well_formed_content('<a><b></a>')";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result
            .expect("XML content well-formedness predicate")
            .rows(),
        &[vec![Value::Bool(true), Value::Bool(false)]]
    );
}

#[test]
fn should_concatenate_xml_documents_in_argument_order() {
    // Arrange
    let fixture = sql_fixture("issue-906-xmlconcat-order", &[]);
    let sql = "SELECT XMLCONCAT('<one/>', '<two/>')::TEXT";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("XMLCONCAT preserves argument order").rows(),
        &[vec![Value::String("<one/><two/>".to_owned())]]
    );
}

#[test]
fn should_preserve_xml_content_fragments() {
    // Arrange
    let fixture = sql_fixture("issue-906-xml-content-fragment", &[]);
    let sql = "SELECT XMLSERIALIZE(CONTENT XMLPARSE(CONTENT 'before<a/>after<b/>') AS TEXT)";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("XML content serialization").rows(),
        &[vec![Value::String("before<a/>after<b/>".to_owned())]]
    );
}

#[test]
fn should_preserve_xml_root_declaration_metadata() {
    // Arrange
    let fixture = sql_fixture("issue-906-xml-document-declarations", &[]);
    let sql = "SELECT XMLSERIALIZE(DOCUMENT XMLPARSE(DOCUMENT '<?xml version=\"1.0\" encoding=\"UTF-8\"?><root/>') AS TEXT), XMLSERIALIZE(CONTENT XMLROOT(XMLPARSE(DOCUMENT '<root/>'), VERSION '1.1', STANDALONE NO) AS TEXT)";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("XML declaration and ROOT metadata").rows(),
        &[vec![
            Value::String("<?xml version=\"1.0\" encoding=\"UTF-8\"?><root/>".to_owned()),
            Value::String("<?xml version=\"1.1\" standalone=\"no\"?><root/>".to_owned()),
        ]]
    );
}

#[test]
fn should_indent_nested_xml_serialization_with_postgresql_whitespace() {
    // Arrange
    let fixture = sql_fixture("issue-906-xml-serialize-indent", &[]);
    let sql = "SELECT XMLSERIALIZE(DOCUMENT XMLPARSE(DOCUMENT '<root><item>one</item><item><child>two</child></item></root>') AS TEXT INDENT)";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("XMLSERIALIZE INDENT output").rows(),
        &[vec![Value::String(
            "<root>\n  <item>one</item>\n  <item>\n    <child>two</child>\n  </item>\n</root>"
                .to_owned()
        )]]
    );
}

#[test]
fn should_escape_xml_element_text_attributes() {
    // Arrange
    let fixture = sql_fixture("issue-906-xml-element-escaping", &[]);
    let sql = "SELECT XMLSERIALIZE(CONTENT XMLELEMENT(NAME item, XMLATTRIBUTES(NULL::text AS omitted, 'a&\"<>' AS quoted), 'text&<', NULL::text, XMLELEMENT(NAME child, 2)) AS TEXT)";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result
            .expect("XMLELEMENT content and attribute escaping")
            .rows(),
        &[vec![Value::String(
            "<item quoted=\"a&amp;&quot;&lt;&gt;\">text&amp;&lt;<child>2</child></item>".to_owned()
        )]]
    );
}

#[test]
fn should_omit_null_xmlforest_fields() {
    // Arrange
    let fixture = sql_fixture("issue-906-xmlforest-null-fields", &[]);
    let sql = "SELECT XMLSERIALIZE(CONTENT XMLFOREST('left&' AS first, NULL::text AS missing, 2 AS number) AS TEXT)";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("XMLFOREST omits NULL values").rows(),
        &[vec![Value::String(
            "<first>left&amp;</first><number>2</number>".to_owned()
        )]]
    );
}

#[test]
fn should_ignore_null_xmlconcat_inputs_while_preserving_order() {
    // Arrange
    let fixture = sql_fixture("issue-906-xmlconcat-null-inputs", &[]);
    let sql = "SELECT XMLCONCAT(NULL::xml, '<one/>', NULL::xml, '<two/>')::text";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("XMLCONCAT ignores NULL values").rows(),
        &[vec![Value::String("<one/><two/>".to_owned())]]
    );
}

#[test]
fn should_map_xpath_prefixes_to_document_namespaces() {
    // Arrange
    let fixture = sql_fixture("issue-906-xpath-prefix-mapping", &[]);
    let sql = "SELECT xpath_exists('/p:root/p:item[text()=\"B\"]', XMLPARSE(DOCUMENT '<root xmlns=\"urn:example\"><item>A</item><item>B</item></root>'), ARRAY[ARRAY['p','urn:example']]), array_length(xpath('/p:root/p:item/text()', XMLPARSE(DOCUMENT '<root xmlns=\"urn:example\"><item>A</item><item>B</item></root>'), ARRAY[ARRAY['p','urn:example']]), 1), xpath_exists('/p:root/p:missing', XMLPARSE(DOCUMENT '<root xmlns=\"urn:example\"><item>A</item></root>'), ARRAY[ARRAY['p','urn:example']])";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("XPath prefix and namespace mapping").rows(),
        &[vec![Value::Bool(true), Value::Int64(2), Value::Bool(false)]]
    );
}

#[test]
fn should_preserve_xpath_null_empty_node_set_semantics() {
    // Arrange
    let fixture = sql_fixture("issue-906-xpath-null-arguments", &[]);
    let sql = "SELECT array_length(xpath('/root/missing', XMLPARSE(DOCUMENT '<root/>')), 1) IS NULL, xpath(NULL::text, XMLPARSE(DOCUMENT '<root/>')) IS NULL, xpath_exists('/root/item', NULL::xml) IS NULL";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result
            .expect("XPath NULL and empty node-set behavior")
            .rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true)
        ]]
    );
}

#[test]
fn should_apply_xmltable_defaults_to_missing_columns() {
    // Arrange
    let fixture = sql_fixture("issue-906-xmltable-defaults", &[]);
    let sql = "SELECT id, value, absent, fallback FROM XMLTABLE('/root/item' PASSING XMLPARSE(DOCUMENT '<root><item id=\"1\">one</item><item id=\"2\"><child>two</child></item></root>') COLUMNS id FOR ORDINALITY, value TEXT PATH 'text()', absent TEXT PATH 'missing/text()', fallback TEXT PATH 'missing/text()' DEFAULT 'fallback') ORDER BY id";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("XMLTABLE column defaults and NULLs").rows(),
        &[
            vec![
                Value::Int64(1),
                Value::String("one".to_owned()),
                Value::Null,
                Value::String("fallback".to_owned()),
            ],
            vec![
                Value::Int64(2),
                Value::Null,
                Value::Null,
                Value::String("fallback".to_owned()),
            ],
        ]
    );
}

#[test]
fn should_return_no_xmltable_rows_when_the_row_path_matches_nothing() {
    // Arrange
    let fixture = sql_fixture("issue-906-xmltable-no-row-match", &[]);
    let sql = "SELECT id, value FROM XMLTABLE('/root/item' PASSING XMLPARSE(DOCUMENT '<root/>') COLUMNS id FOR ORDINALITY, value TEXT PATH 'text()')";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert!(
        result.expect("XMLTABLE empty row set").rows().is_empty(),
        "an unmatched row path must produce zero rows"
    );
}

#[test]
fn should_preserve_external_xml_entity_references_without_loading_resources() {
    // Arrange
    let fixture = sql_fixture("issue-906-xml-external-entity", &[]);
    let sql = "SELECT XMLSERIALIZE(DOCUMENT XMLPARSE(DOCUMENT '<!DOCTYPE root [<!ENTITY pg18_external SYSTEM \"file:///definitely-not-present/cassie-xml-external-entity\">]><root>&pg18_external;</root>') AS TEXT)";

    // Act
    let result = fixture.execute(sql);

    // Assert
    assert_eq!(
        result.expect("external entity is retained without expansion").rows(),
        &[vec![Value::String(
            "<!DOCTYPE root [<!ENTITY pg18_external SYSTEM \"file:///definitely-not-present/cassie-xml-external-entity\">]><root>&pg18_external;</root>"
                .to_owned()
        )]]
    );
}
