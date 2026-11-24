#[test]
fn should_preserve_jsonb_array_index_null_boundaries() {
    // Arrange
    let setup = [
        (
            "CREATE TABLE issue_809_jsonb_array_index_values (document JSONB)",
            Vec::new(),
        ),
        (
            "INSERT INTO issue_809_jsonb_array_index_values VALUES ('[\"zero\",null,\"two\"]')",
            Vec::new(),
        ),
    ];
    let cycle = fixture::execute_cycle(
        "SELECT document -> 1, document ->> -1, document -> 5, document ->> 5 \
         FROM issue_809_jsonb_array_index_values",
        None,
        0,
        true,
    );

    // Act
    let batches = fixture::run_cycles("issue-809-jsonb-array-index-null", &setup, vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONB array index RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![3802, 25, 3802, 25]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some("null".to_owned()),
            Some("two".to_owned()),
            None,
            None,
        ]]
    );
}
