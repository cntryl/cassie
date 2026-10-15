use super::binary_data_rows;
use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

#[test]
fn should_describe_xmltable_column_types_over_text_pgwire() {
    // Arrange
    let sql = "SELECT ordinal, amount, fragment FROM XMLTABLE(\
        '/root/item' PASSING XMLPARSE(DOCUMENT \
        '<root><item><amount>12.50</amount><fragment><token>é</token></fragment></item></root>') \
        COLUMNS ordinal FOR ORDINALITY, amount NUMERIC PATH 'amount/text()', \
        fragment XML PATH 'fragment') AS expanded";
    let steps = vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])];

    // Act
    let batches = run_script("issue-906-xmltable-column-descriptors", steps);

    // Assert
    let result = &batches[0];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("XMLTABLE RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| (
                column.type_oid,
                column.type_size,
                column.type_mod,
                column.format_code,
            ))
            .collect::<Vec<_>>(),
        vec![(23, 4, -1, 0), (1700, -1, -1, 0), (142, -1, -1, 0)]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![
            Some("1".to_owned()),
            Some("12.50".to_owned()),
            Some("<fragment><token>é</token></fragment>".to_owned()),
        ]]
    );
}

#[test]
fn should_encode_xmltable_xml_columns_in_binary_pgwire_results() {
    // Arrange
    let sql = "SELECT fragment FROM XMLTABLE(\
        '/root/item' PASSING XMLPARSE(DOCUMENT \
        '<root><item><fragment><token>é</token></fragment></item></root>') \
        COLUMNS fragment XML PATH 'fragment') AS expanded";
    let steps = vec![
        Step::ParseFlush(wire::parse_frame("xmltable_binary_output", sql)),
        Step::ThroughReady(vec![
            wire::bind_frame_with_formats(
                "xmltable_binary_output_portal",
                "xmltable_binary_output",
                &[],
                &[],
                &[1],
            ),
            wire::describe_portal_frame("xmltable_binary_output_portal"),
            wire::execute_frame("xmltable_binary_output_portal"),
            wire::sync_frame(),
        ]),
    ];

    // Act
    let batches = run_script("issue-906-xmltable-binary-output", steps);

    // Assert
    assert_eq!(wire::error_code(&batches[0]), None);
    assert_eq!(batches[0].first().map(|(tag, _)| *tag), Some(b'1'));
    let result = &batches[1];
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("XMLTABLE binary RowDescription");
    let column = &wire::parse_row_description(&description.1)[0];
    assert_eq!(
        (
            column.type_oid,
            column.type_size,
            column.type_mod,
            column.format_code,
        ),
        (142, -1, -1, 1)
    );
    assert_eq!(
        binary_data_rows(result),
        vec![vec![Some(
            "<fragment><token>é</token></fragment>".as_bytes().to_vec()
        )]]
    );
}
