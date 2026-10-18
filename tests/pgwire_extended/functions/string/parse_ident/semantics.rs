use super::{run_script, wire, Step};

#[test]
fn should_decode_quoted_identifier_components() {
    // Arrange
    let batches = run_script(
        "issue-803-parse-ident-quoted-escapes",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            r#"SELECT parse_ident('"sch"."table.with.dot"."a""b"')::TEXT AS identifiers"#,
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("parse_ident RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 25);
    assert_eq!(
        wire::data_rows(result),
        vec![vec![Some(r#"{sch,table.with.dot,"a\"b"}"#.to_owned())]]
    );
}
