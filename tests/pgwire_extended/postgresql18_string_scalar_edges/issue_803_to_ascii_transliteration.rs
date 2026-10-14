use super::{error_fields, run_script, wire, Step};

#[test]
fn should_transliterate_latin1_text_over_pgwire() {
    // Arrange
    let batches = run_script(
        "issue-803-to-ascii-latin1-wire",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT to_ascii('äéñ', 'LATIN1') AS by_name, to_ascii('äéñ', 8) AS by_id",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(
        wire::error_code(result),
        None,
        "to_ascii errors: {:?}",
        error_fields(result)
    );
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("to_ascii RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(25, -1, -1), (25, -1, -1)]
    );
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some("A ACA ".to_owned()), Some("A ACA ".to_owned())]]
    );
}
