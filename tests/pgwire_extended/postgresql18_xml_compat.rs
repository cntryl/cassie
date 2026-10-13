//! PostgreSQL 18 XML type and parser errors tracked by #906.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_preserve_postgresql_xml_type_oid_for_document_results() {
    // Arrange
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT XMLPARSE(DOCUMENT '<root><item>é</item></root>') AS document",
    )])];

    // Act
    let batches = run_script("postgresql18-xml-document-result", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    let description = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("XML RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 142);
    assert_eq!(
        wire::data_rows(&batches[0]),
        vec![vec![Some("<root><item>é</item></root>".to_owned())]]
    );
}

#[test]
fn should_report_malformed_xml_with_the_xml_document_sqlstate() {
    // Arrange
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT XMLPARSE(DOCUMENT '<root>')",
    )])];

    // Act
    let batches = run_script("postgresql18-xml-malformed-document", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("2200M"));
}

#[test]
fn should_report_invalid_xpath_sqlstate_for_xpath_entrypoints() {
    // Arrange
    let invalid_xpath = "SELECT xpath('//*[', XMLPARSE(DOCUMENT '<root/>'))";
    let invalid_xmltable_path = "SELECT * FROM XMLTABLE('//*[' PASSING XMLPARSE(DOCUMENT '<root/>') COLUMNS value TEXT PATH 'text()')";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(invalid_xpath)]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 41")]),
        Step::ThroughReady(vec![wire::simple_query_frame(invalid_xmltable_path)]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 42")]),
    ];

    // Act
    let batches = run_script("postgresql18-xml-invalid-xpath", steps);

    // Assert
    assert_eq!(batches.len(), 4);
    // PostgreSQL 18.6 reports libxml invalid-expression errors as SQLSTATE 10608.
    for error in [&batches[0], &batches[2]] {
        assert_eq!(wire::error_code(error).as_deref(), Some("10608"));
        assert_eq!(wire::data_rows(error), Vec::<Vec<Option<String>>>::new());
    }
    for (frames, expected) in [(&batches[1], "41"), (&batches[3], "42")] {
        assert_eq!(wire::error_code(frames), None);
        assert_eq!(
            wire::data_rows(frames),
            vec![vec![Some(expected.to_owned())]]
        );
    }
}
