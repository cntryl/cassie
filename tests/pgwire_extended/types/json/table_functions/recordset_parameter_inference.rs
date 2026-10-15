//! Unspecified pgwire parameters infer JSONB from JSON recordset calls.

use super::super::{fixture, wire};

#[test]
fn should_infer_jsonb_recordset_parameter_type() {
    // Arrange
    let sql = "SELECT item.id, item.amount, item.label FROM jsonb_to_recordset($1) AS item(id INTEGER, amount NUMERIC(8,2), label VARCHAR(5)) ORDER BY item.id";
    let document =
        r#"[{"id":7,"amount":12.345,"label":"Ada"},{"id":8,"amount":-0.004,"label":null}]"#;
    let cycle = vec![
        wire::parse_frame_with_types("recordset_parameter", sql, &[0]),
        wire::describe_statement_frame("recordset_parameter"),
        wire::bind_frame("recordset_portal", "recordset_parameter", &[document]),
        wire::describe_portal_frame("recordset_portal"),
        wire::execute_frame("recordset_portal"),
        wire::sync_frame(),
    ];

    // Act
    let batches = fixture::run_cycles("json-recordset-inferred-parameter", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let parameter_description = frames
        .iter()
        .find(|(tag, _)| *tag == b't')
        .expect("inferred JSONB ParameterDescription");
    assert_eq!(
        wire::parse_parameter_description(&parameter_description.1),
        vec![3_802]
    );

    let row_descriptions = frames
        .iter()
        .filter(|(tag, _)| *tag == b'T')
        .map(|(_, body)| wire::parse_row_description(body))
        .collect::<Vec<_>>();
    assert_eq!(row_descriptions.len(), 2);
    for columns in row_descriptions {
        assert_eq!(
            columns
                .iter()
                .map(|column| (column.type_oid, column.type_size, column.type_mod))
                .collect::<Vec<_>>(),
            vec![(23, 4, -1), (1_700, -1, 524_294), (1_043, -1, 9)]
        );
    }
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![
                Some("7".to_owned()),
                Some("12.35".to_owned()),
                Some("Ada".to_owned()),
            ],
            vec![Some("8".to_owned()), Some("0.00".to_owned()), None,],
        ]
    );
}
