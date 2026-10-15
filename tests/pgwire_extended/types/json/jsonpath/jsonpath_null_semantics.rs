use super::*;

#[test]
fn should_report_jsonpath_null_path_truth_table() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_jsonpath_null_values (id INT4 PRIMARY KEY, document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_jsonpath_null_values VALUES (1, '{\"a\":null}'), (2, '{\"a\":1}'), (3, '{}'), (4, NULL)",
            Vec::new(),
        ),
    ];
    let sql = "SELECT id, document @? '$.a ? (@ == null)'::JSONPATH AS has_document_null, document @@ '$.a == null'::JSONPATH AS matches_document_null FROM issue_809_jsonpath_null_values ORDER BY id";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-809-jsonpath-null-distinction", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONPath document NULL RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(23, 4, -1), (16, 1, -1), (16, 1, -1)]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![
            vec![
                Some("1".to_owned()),
                Some("t".to_owned()),
                Some("t".to_owned())
            ],
            vec![
                Some("2".to_owned()),
                Some("f".to_owned()),
                Some("f".to_owned())
            ],
            vec![
                Some("3".to_owned()),
                Some("f".to_owned()),
                Some("f".to_owned())
            ],
            vec![Some("4".to_owned()), None, None],
        ]
    );
}
