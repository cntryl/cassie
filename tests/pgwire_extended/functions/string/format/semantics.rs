#[test]
fn should_apply_positional_dynamic_width_format_specifiers() {
    // Arrange
    let sql = "SELECT format('[%2$-*1$s]|[%4$*3$s]|[%5$I]|[%6$L]', 8, 'cat', 4, 'x', 'select', 'O''Reilly') AS formatted";

    // Act
    let batches = run_script(
        "issue-803-format-positional-dynamic-width",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(sql)])],
    );
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None, "format errors: {result:?}");
    let description = result
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("format RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 25);
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some(
            "[cat     ]|[   x]|[\"select\"]|['O''Reilly']".to_owned()
        )]]
    );
}
