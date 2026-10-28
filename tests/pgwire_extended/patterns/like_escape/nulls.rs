use super::{run_script, wire, Step};

#[test]
fn should_return_null_for_null_like_escape() {
    // Arrange
    let batches = run_script(
        "issue-799-like-null-escape",
        vec![Step::ThroughReady(vec![wire::simple_query_frame(
            "SELECT 'abc' LIKE 'a%' ESCAPE NULL::TEXT AS matched",
        )])],
    );

    // Act
    let result = &batches[0];

    // Assert
    assert_eq!(wire::error_code(result), None);
    let description = result
        .iter()
        .find(|frame| frame.0 == b'T')
        .expect("LIKE result RowDescription");
    assert_eq!(wire::parse_row_description(&description.1)[0].type_oid, 16);
    assert_eq!(wire::data_rows(result), vec![vec![None]]);
}
