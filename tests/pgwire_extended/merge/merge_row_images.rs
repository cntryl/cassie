use super::super::{support_pgwire as wire, support_type_metadata_contract as fixture};

#[test]
fn should_return_action_specific_merge_row_images_with_pgwire_descriptors() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE merge_old_new_target (id INT PRIMARY KEY, label TEXT)",
            Vec::new(),
        ),
        (
            "CREATE TABLE merge_old_new_source (id INT, action TEXT, label TEXT)",
            Vec::new(),
        ),
        (
            "INSERT INTO merge_old_new_target VALUES (1, 'before update'), (2, 'before delete')",
            Vec::new(),
        ),
        (
            "INSERT INTO merge_old_new_source VALUES \
                (1, 'UPDATE', 'after update'), \
                (2, 'DELETE', 'unused'), \
                (3, 'INSERT', 'inserted')",
            Vec::new(),
        ),
    ];
    let sql = "MERGE INTO merge_old_new_target AS target \
         USING merge_old_new_source AS source ON target.id = source.id \
         WHEN MATCHED AND source.action = 'UPDATE' THEN UPDATE SET label = source.label \
         WHEN MATCHED AND source.action = 'DELETE' THEN DELETE \
         WHEN NOT MATCHED BY TARGET AND source.action = 'INSERT' \
           THEN INSERT (id, label) VALUES (source.id, source.label) \
         RETURNING WITH (OLD AS old_row, NEW AS new_row) \
           merge_action() AS action, \
           old_row.id AS old_id, old_row.label AS old_label, \
           new_row.id AS new_id, new_row.label AS new_label";
    let cycles = vec![vec![wire::simple_query_frame(sql)]];

    // Act
    let batches = fixture::run_cycles("merge-old-new-row-images-pgwire", &setup, cycles);
    let frames = &batches[0];

    // Assert
    assert_eq!(
        wire::error_code(frames),
        None,
        "wire errors: {:?}",
        frames
            .iter()
            .filter(|(tag, _)| *tag == b'E')
            .map(|(_, body)| wire::parse_error_fields(body))
            .collect::<Vec<_>>()
    );
    let row_description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("MERGE RowDescription");
    let columns = wire::parse_row_description(&row_description.1);
    assert_eq!(
        columns
            .iter()
            .map(|column| (
                column.name.as_str(),
                column.type_oid,
                column.type_size,
                column.type_mod,
                column.format_code,
            ))
            .collect::<Vec<_>>(),
        vec![
            ("action", 25, -1, -1, 0),
            ("old_id", 23, 4, -1, 0),
            ("old_label", 25, -1, -1, 0),
            ("new_id", 23, 4, -1, 0),
            ("new_label", 25, -1, -1, 0),
        ]
    );
    let mut images = wire::data_rows(frames);
    images.sort_by_key(|row| row[0].clone());
    assert_eq!(
        images,
        vec![
            vec![
                Some("DELETE".to_owned()),
                Some("2".to_owned()),
                Some("before delete".to_owned()),
                None,
                None,
            ],
            vec![
                Some("INSERT".to_owned()),
                None,
                None,
                Some("3".to_owned()),
                Some("inserted".to_owned()),
            ],
            vec![
                Some("UPDATE".to_owned()),
                Some("1".to_owned()),
                Some("before update".to_owned()),
                Some("1".to_owned()),
                Some("after update".to_owned()),
            ],
        ]
    );
}
