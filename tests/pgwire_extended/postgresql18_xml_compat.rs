//! PostgreSQL 18 XML type and parser errors tracked by #906.

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
