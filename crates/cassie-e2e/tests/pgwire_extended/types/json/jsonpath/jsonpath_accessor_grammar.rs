//! PostgreSQL 18.6 quoted and wildcard `JSONPath` accessor semantics for #809.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

const JSON_DOCUMENT: &str = r#"{"field.with.dot":{"quoted key":7},"object":{"z":3,"a":1},"tree":{"v":0,"child":{"v":1,"grandchild":{"v":2,"great":{"v":3}}}}}"#;

fn run_jsonpath_predicate(label: &str, path: &str) -> fixture::Frames {
    let insert_sql =
        format!("INSERT INTO issue_809_jsonpath_accessor_grammar VALUES ('{JSON_DOCUMENT}')");
    let setup = [
        (
            "CREATE TABLE issue_809_jsonpath_accessor_grammar (document JSONB)",
            Vec::new(),
        ),
        (insert_sql.as_str(), Vec::new()),
    ];
    let sql = format!("SELECT document @? '{path}' FROM issue_809_jsonpath_accessor_grammar");
    let cycle = fixture::execute_cycle(&sql, None, 0, true);
    let batches = fixture::run_cycles(label, &setup, vec![cycle]);
    batches
        .into_iter()
        .next()
        .expect("one JSONPath predicate result")
}

fn assert_jsonpath_match(frames: &fixture::Frames) {
    fixture::assert_success(frames);
    let description = frames
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("JSONPath predicate RowDescription");
    assert_eq!(
        wire::parse_row_description(&description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![16]
    );
    assert_eq!(wire::data_rows(frames), vec![vec![Some("t".to_owned())]]);
}

#[test]
fn should_match_jsonpath_quoted_object_keys() {
    // Arrange
    let path = r#"$."field.with.dot"."quoted key" ? (@ == 7)"#;

    // Act
    let frames = run_jsonpath_predicate("issue-809-jsonpath-quoted-keys", path);

    // Assert
    assert_jsonpath_match(&frames);
}

#[test]
fn should_match_jsonpath_object_wildcards() {
    // Arrange
    let path = "$.object.* ? (@ == 3)";

    // Act
    let frames = run_jsonpath_predicate("issue-809-jsonpath-object-wildcard", path);

    // Assert
    assert_jsonpath_match(&frames);
}

#[test]
fn should_match_jsonpath_bounded_recursive_accessors() {
    // Arrange
    let path = "$.tree.**{1 to 2}.v ? (@ == 2)";

    // Act
    let frames = run_jsonpath_predicate("issue-809-jsonpath-bounded-recursion", path);

    // Assert
    assert_jsonpath_match(&frames);
}
