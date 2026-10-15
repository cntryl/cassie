//! JSON and JSONB declared recordset fields retain distinct pgwire types.

use super::super::{fixture, wire};

#[test]
fn should_preserve_distinct_document_recordset_output_type_identities() {
    // Arrange
    let sql = r#"SELECT item.json_value, item.jsonb_value
        FROM jsonb_to_recordset('[{"json_value":{"k":[1,2]},"jsonb_value":{"k":[1,2]}}]'::JSONB)
             AS item(json_value JSON, jsonb_value JSONB)"#;
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles(
        "issue-829-recordset-json-jsonb-output-types",
        &[],
        vec![cycle],
    );

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONB recordset RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        [(114, -1, -1), (3_802, -1, -1)]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some(r#"{"k": [1, 2]}"#.to_owned()),
            Some(r#"{"k": [1, 2]}"#.to_owned()),
        ]]
    );
}
