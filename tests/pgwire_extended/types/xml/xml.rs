//! PostgreSQL 18 XML type and parser errors tracked by #906.

#[path = "xmltable_cancellation.rs"]
mod xmltable_cancellation;

#[path = "xmltable_wire_typing.rs"]
mod xmltable_wire_typing;

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

fn binary_data_rows(frames: &[(u8, Vec<u8>)]) -> Vec<Vec<Option<Vec<u8>>>> {
    frames
        .iter()
        .filter(|(tag, _)| *tag == b'D')
        .map(|(_, payload)| {
            let field_count = i16::from_be_bytes(
                payload[..2]
                    .try_into()
                    .expect("DataRow field count should be present"),
            );
            let mut cursor = 2_usize;
            (0..field_count)
                .map(|_| {
                    let length = i32::from_be_bytes(
                        payload[cursor..cursor + 4]
                            .try_into()
                            .expect("DataRow field length should be present"),
                    );
                    cursor += 4;
                    if length < 0 {
                        None
                    } else {
                        let end = cursor
                            + usize::try_from(length).expect("field length should fit usize");
                        let value = payload[cursor..end].to_vec();
                        cursor = end;
                        Some(value)
                    }
                })
                .collect()
        })
        .collect()
}

fn xml_array_binary_payload(values: &[Option<&str>]) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&1_i32.to_be_bytes());
    let has_null = i32::from(values.iter().any(Option::is_none));
    payload.extend_from_slice(&has_null.to_be_bytes());
    payload.extend_from_slice(&142_i32.to_be_bytes());
    payload.extend_from_slice(
        &i32::try_from(values.len())
            .expect("XML array length should fit i32")
            .to_be_bytes(),
    );
    payload.extend_from_slice(&1_i32.to_be_bytes());
    for value in values {
        if let Some(value) = value {
            payload.extend_from_slice(
                &i32::try_from(value.len())
                    .expect("XML value length should fit i32")
                    .to_be_bytes(),
            );
            payload.extend_from_slice(value.as_bytes());
        } else {
            payload.extend_from_slice(&(-1_i32).to_be_bytes());
        }
    }
    payload
}

#[test]
fn should_cast_xml_text_to_a_document_with_postgresql_identity() {
    // Arrange
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT '<root/>'::xml AS document",
    )])];

    // Act
    let batches = run_script("postgresql18-xml-text-cast-identity", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("XML RowDescription");
    let column = &wire::parse_row_description(&description.1)[0];
    assert_eq!(
        (column.type_oid, column.type_size, column.type_mod),
        (142, -1, -1)
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("<root/>".to_owned())]]
    );
}

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
fn should_decode_binary_xml_parameters_as_documents() {
    // Arrange
    let xml_binary = "<root>é</root>".as_bytes();
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "xml_binary_input",
            "SELECT $1 AS document",
            &[142],
        )),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "xml_binary_input_portal",
                "xml_binary_input",
                &[1],
                &[Some(xml_binary)],
                &[0],
            ),
            wire::describe_portal_frame("xml_binary_input_portal"),
            wire::execute_frame("xml_binary_input_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-906-xml-binary-input", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(batches[0].first().map(|(tag, _)| *tag), Some(b'1'));
    let result = &batches[1];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("XML binary-input RowDescription");
    let column = &wire::parse_row_description(&description.1)[0];
    assert_eq!((column.type_oid, column.format_code), (142, 0));
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("<root>é</root>".to_owned())]]
    );
}

#[test]
fn should_encode_binary_xml_results_with_postgresql_metadata() {
    // Arrange
    let sql = "SELECT '<root><item id=\"7\">é</item></root>'::xml AS document";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("xml_binary_output", sql)),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "xml_binary_output_portal",
                "xml_binary_output",
                &[],
                &[],
                &[1],
            ),
            wire::describe_portal_frame("xml_binary_output_portal"),
            wire::execute_frame("xml_binary_output_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-906-xml-binary-output", steps);

    // Assert
    assert_eq!(
        wire::error_code(&batches[0]),
        None,
        "ParseFlush error: {:?}",
        batches[0]
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, payload)| wire::parse_error_fields(payload))
            .collect::<Vec<_>>()
    );
    assert_eq!(batches[0].first().map(|(tag, _)| *tag), Some(b'1'));
    let result = &batches[1];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("XML binary-output RowDescription");
    let column = &wire::parse_row_description(&description.1)[0];
    assert_eq!(
        (
            column.type_oid,
            column.type_size,
            column.type_mod,
            column.format_code
        ),
        (142, -1, -1, 1)
    );
    // PostgreSQL 18.6 xml_send emits the UTF-8 XML document bytes verbatim.
    assert_eq!(
        binary_data_rows(result),
        vec![vec![Some(
            "<root><item id=\"7\">é</item></root>".as_bytes().to_vec()
        )]]
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

#[test]
fn should_preserve_xml_array_binary_identity() {
    // Arrange
    let expected = xml_array_binary_payload(&[Some("<a/>"), None, Some("<b/>")]);
    let steps = vec![
        Step::ParseFlush(wire::parse_frame_with_types(
            "xml_array_binary_input",
            "SELECT $1::xml[] AS documents",
            &[143],
        )),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "xml_array_binary_input_portal",
                "xml_array_binary_input",
                &[1],
                &[Some(expected.as_slice())],
                &[1],
            ),
            wire::describe_portal_frame("xml_array_binary_input_portal"),
            wire::execute_frame("xml_array_binary_input_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-906-xml-array-binary-identity", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(batches[0].first().map(|(tag, _)| *tag), Some(b'1'));
    let result = &batches[1];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("XML array RowDescription");
    let column = &wire::parse_row_description(&description.1)[0];
    assert_eq!(
        (
            column.type_oid,
            column.type_size,
            column.type_mod,
            column.format_code
        ),
        (143, -1, -1, 1)
    );
    assert_eq!(binary_data_rows(result), vec![vec![Some(expected)]]);
}

#[test]
fn should_apply_xmloption_cast_rules() {
    // Arrange
    let content_cast = "SELECT 'before<a/>after<b/>'::xml AS fragment";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame("SET xmloption = DOCUMENT")]),
        Step::ThroughReady(vec![wire::simple_query_frame(content_cast)]),
        Step::ThroughReady(vec![wire::simple_query_frame("SET xmloption = CONTENT")]),
        Step::ThroughReady(vec![wire::simple_query_frame(content_cast)]),
    ];

    // Act
    let batches = run_script("issue-906-xmloption-session-setting", steps);

    // Assert
    assert_eq!(batches.len(), 4);
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(wire::error_code(&batches[1]).as_deref(), Some("2200M"));
    assert_eq!(
        wire::data_rows(&batches[1]),
        Vec::<Vec<Option<String>>>::new()
    );
    assert_eq!(wire::error_code(&batches[2]), None);
    assert_eq!(wire::error_code(&batches[3]), None);
    let content_description = batches[3]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("XML content-cast RowDescription");
    assert_eq!(
        wire::parse_row_description(&content_description.1)[0].type_oid,
        142
    );
    assert_eq!(
        wire::data_rows(&batches[3]),
        vec![vec![Some("before<a/>after<b/>".to_owned())]]
    );
}

#[test]
fn should_preserve_xml_oid_for_sql_null_values() {
    // Arrange
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT NULL::xml AS missing",
    )])];

    // Act
    let batches = run_script("issue-906-xml-null-value", steps);

    // Assert
    assert_eq!(batches.len(), 1);
    assert_eq!(wire::error_code(&batches[0]), None);
    let null_description = batches[0]
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("XML NULL RowDescription");
    assert_eq!(
        wire::parse_row_description(&null_description.1)[0].type_oid,
        142
    );
    assert_eq!(wire::data_rows(&batches[0]), vec![vec![None]]);
}

#[test]
fn should_report_invalid_xml_content_with_its_sqlstate() {
    // Arrange
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT XMLPARSE(CONTENT '<a><b></a>')",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 41")]),
    ];

    // Act
    let batches = run_script("issue-906-xml-invalid-content", steps);

    // Assert
    assert_eq!(batches.len(), 2);
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("2200N"));
    assert_eq!(
        wire::data_rows(&batches[0]),
        Vec::<Vec<Option<String>>>::new()
    );
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("41".to_owned())]]
    );
}

#[test]
fn should_reject_xml_documents_with_multiple_roots() {
    // Arrange
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT XMLPARSE(DOCUMENT '<a/><b/>')",
    )])];

    // Act
    let batches = run_script("issue-906-xml-multiple-document-roots", steps);

    // Assert
    assert_eq!(batches.len(), 1);
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("2200M"));
    assert_eq!(
        wire::data_rows(&batches[0]),
        Vec::<Vec<Option<String>>>::new()
    );
}

#[test]
fn should_report_xml_equality_as_an_undefined_operator() {
    // Arrange
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(
        "SELECT '<root/>'::xml = '<root/>'::xml",
    )])];

    // Act
    let batches = run_script("issue-906-xml-no-equality-operator", steps);

    // Assert
    assert_eq!(batches.len(), 1);
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("42883"));
    assert_eq!(
        wire::data_rows(&batches[0]),
        Vec::<Vec<Option<String>>>::new()
    );
}

#[test]
fn should_report_xmltable_typed_conversion_sqlstate_without_partial_rows() {
    // Arrange
    let invalid_conversion = "SELECT * FROM XMLTABLE('/root/item' PASSING XMLPARSE(DOCUMENT '<root><item>1</item><item>bad</item></root>') COLUMNS value integer PATH 'text()')";
    let steps = vec![
        Step::ThroughReady(vec![wire::simple_query_frame(invalid_conversion)]),
        Step::ThroughReady(vec![wire::simple_query_frame("SELECT 44")]),
    ];

    // Act
    let batches = run_script("issue-906-xmltable-conversion-error", steps);

    // Assert
    assert_eq!(batches.len(), 2);
    assert_eq!(wire::error_code(&batches[0]).as_deref(), Some("22P02"));
    assert_eq!(
        wire::data_rows(&batches[0]),
        Vec::<Vec<Option<String>>>::new()
    );
    assert_eq!(wire::error_code(&batches[1]), None);
    assert_eq!(
        wire::data_rows(&batches[1]),
        vec![vec![Some("44".to_owned())]]
    );
}
