//! Declared typmods on JSON recordset columns remain visible over pgwire.

use super::super::{fixture, wire};

#[test]
fn should_preserve_declared_json_recordset_column_typmods() {
    // Arrange
    let document =
        r#"[{"id":7,"amount":12.345,"label":"Ada"},{"id":null,"amount":-0.004,"label":null}]"#;
    let cycle = fixture::execute_cycle(
        &format!(
            "SELECT id, amount, label FROM jsonb_to_recordset('{document}'::JSONB) AS records(id INTEGER, amount NUMERIC(8,2), label VARCHAR(5))"
        ),
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-829-json-recordset-output-typmods", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("jsonb_to_recordset RowDescription");
    let columns = wire::parse_row_description(&description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(23, 4, -1), (1_700, -1, 524_294), (1_043, -1, 9)]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![
                Some("7".to_owned()),
                Some("12.35".to_owned()),
                Some("Ada".to_owned()),
            ],
            vec![None, Some("0.00".to_owned()), None],
        ]
    );
}
