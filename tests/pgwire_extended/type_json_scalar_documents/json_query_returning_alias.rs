use super::*;

#[test]
fn should_preserve_json_query_returning_type_identity() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_810_json_query_returning_alias (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_810_json_query_returning_alias VALUES ('{\"items\":[1,2]}')",
            Vec::new(),
        ),
    ];
    let sql = "SELECT JSON_QUERY(document, '$.items' RETURNING JSON) AS json_result, JSON_QUERY(document, '$.items' RETURNING JSONB) AS jsonb_result FROM issue_810_json_query_returning_alias";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("issue-810-json-query-returning-alias", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSON_QUERY RETURNING type RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| (column.type_oid, column.type_size, column.type_mod))
            .collect::<Vec<_>>(),
        vec![(114, -1, -1), (3_802, -1, -1)]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![Some("[1, 2]".to_owned()), Some("[1, 2]".to_owned())]]
    );
}
