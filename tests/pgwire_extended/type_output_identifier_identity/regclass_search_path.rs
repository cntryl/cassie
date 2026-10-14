//! Search-path lookup order for `regclass` values over PostgreSQL wire.

use crate::support_parameter_occurrence_wire::{Step, run_script};
use crate::support_pgwire as wire;

#[test]
fn should_resolve_regclass_to_first_search_path_relation_over_pgwire() {
    // Arrange
    let setup = [
        "CREATE SCHEMA issue_818_wire_search_a",
        "CREATE SCHEMA issue_818_wire_search_b",
        "CREATE TABLE issue_818_wire_search_a.item (id INT)",
        "CREATE TABLE issue_818_wire_search_b.item (id INT)",
    ];
    let query = "SELECT to_regclass('item') AS selected_relation, \
        to_regclass('item') = to_regclass('issue_818_wire_search_a.item') AS resolves_a, \
        to_regclass('item') = to_regclass('issue_818_wire_search_b.item') AS resolves_b";
    let mut steps = setup
        .iter()
        .map(|sql| Step::ThroughReady(vec![wire::simple_query_frame(sql)]))
        .collect::<Vec<_>>();
    steps.extend([
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SET search_path TO issue_818_wire_search_a, issue_818_wire_search_b, public",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(query)]),
        Step::ThroughReady(vec![wire::simple_query_frame(
            "SET search_path TO issue_818_wire_search_b, issue_818_wire_search_a, public",
        )]),
        Step::ThroughReady(vec![wire::simple_query_frame(query)]),
    ]);

    // Act
    let batches = run_script("issue-818-regclass-search-path-wire", steps);
    let first_search_path = &batches[5];
    let reversed_search_path = &batches[7];

    // Assert
    assert_eq!(batches.len(), 8);
    assert!(
        batches[..5]
            .iter()
            .all(|batch| wire::error_code(batch).is_none()),
        "schema, table, and first search-path setup must succeed"
    );
    assert_eq!(wire::error_code(first_search_path), None);
    let first_description = first_search_path
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("first search-path RowDescription");
    assert_eq!(
        wire::parse_row_description(&first_description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![2205, 16, 16]
    );
    assert_eq!(
        wire::data_rows(first_search_path),
        vec![vec![
            Some("item".to_owned()),
            Some("t".to_owned()),
            Some("f".to_owned()),
        ]]
    );
    assert_eq!(wire::error_code(&batches[6]), None);
    assert_eq!(wire::error_code(reversed_search_path), None);
    let reversed_description = reversed_search_path
        .iter()
        .find(|(tag, _)| *tag == b'T')
        .expect("reversed search-path RowDescription");
    assert_eq!(
        wire::parse_row_description(&reversed_description.1)
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![2205, 16, 16]
    );
    assert_eq!(
        wire::data_rows(reversed_search_path),
        vec![vec![
            Some("item".to_owned()),
            Some("f".to_owned()),
            Some("t".to_owned()),
        ]]
    );
}
