#[test]
fn should_construct_sql_json_values_with_null_policy() {
    // Arrange
    let sql = "SELECT JSON_OBJECT('a' VALUE 1, 'b' VALUE NULL ABSENT ON NULL), JSON_ARRAY(1, NULL, 2 ABSENT ON NULL), JSON_SCALAR('hello')";
    let cycle = fixture::execute_cycle(sql, None, 0, true);

    // Act
    let batches = fixture::run_cycles("type-sql-json-constructors", &[], vec![cycle]);

    // Assert
    let frames = &batches[0];
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("SQL/JSON constructor RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![114, 114, 114]
    );
    assert_eq!(
        wire::data_rows(frames),
        vec![vec![
            Some(r#"{"a" : 1}"#.to_owned()),
            Some("[1, 2]".to_owned()),
            Some(r#""hello""#.to_owned()),
        ]]
    );
}
