//! JSON_TABLE UTF8 bytea output and nested path aliases over pgwire.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

fn column_type_oids(frames: &fixture::Frames) -> Vec<i32> {
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("RowDescription");
    wire::parse_row_description(&description.1)
        .iter()
        .map(|column| column.type_oid)
        .collect()
}

#[test]
fn should_encode_json_table_json_bytes_as_utf8() {
    // Arrange
    let sql = r#"SELECT id, payload FROM JSON_TABLE(
        '{"items":[{"id":1,"payload":{"k":[1,2]}},{"id":2,"payload":null}]}'::JSONB,
        '$.items[*]' COLUMNS (
            id INTEGER PATH '$.id',
            payload BYTEA FORMAT JSON ENCODING UTF8 PATH '$.payload'
        )
    ) AS json_rows ORDER BY id"#;
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-829-json-table-format-json-utf8", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(column_type_oids(frames), vec![23, 17]);
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![
                Some("1".to_owned()),
                Some(r"\x7b226b223a205b312c20325d7d".to_owned()),
            ],
            vec![Some("2".to_owned()), Some(r"\x6e756c6c".to_owned())],
        ]
    );
}

#[test]
fn should_expand_nested_json_table_rows_through_named_path_aliases() {
    // Arrange
    let sql = r#"SELECT parent_id, item_ordinal, label
        FROM JSON_TABLE(
            '[{"id":1,"items":[{"label":"Ada"},{"label":"Linus"}]},{"id":2,"items":[{"label":"Grace"}]}]'::JSONB,
            '$[*]' AS root_path
            COLUMNS (
                parent_id INTEGER PATH '$.id',
                NESTED PATH '$.items[*]' AS item_path COLUMNS (
                    item_ordinal FOR ORDINALITY,
                    label TEXT PATH '$.label'
                )
            )
        ) AS expanded
        ORDER BY parent_id, item_ordinal"#;
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-829-json-table-nested-path-alias", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    assert_eq!(column_type_oids(frames), vec![23, 20, 25]);
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![
                Some("1".to_owned()),
                Some("1".to_owned()),
                Some("Ada".to_owned()),
            ],
            vec![
                Some("1".to_owned()),
                Some("2".to_owned()),
                Some("Linus".to_owned()),
            ],
            vec![
                Some("2".to_owned()),
                Some("1".to_owned()),
                Some("Grace".to_owned()),
            ],
        ]
    );
}
