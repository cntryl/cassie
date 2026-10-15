use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_filter_table_rows_with_text_search_predicates() {
    // Arrange
    let fixture = sql_fixture(
        "issue-799-table-text-search-predicate",
        &[
            "CREATE TABLE issue_799_search_rows (id INT4, document TEXT)",
            "INSERT INTO issue_799_search_rows VALUES \
                 (1, 'fat cats ate rats'), \
                 (2, 'fat cats'), \
                 (3, 'fat cats ate mice')",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT id FROM issue_799_search_rows \
         WHERE to_tsvector('english', document) @@ to_tsquery('english', 'cat & rat') \
         ORDER BY id",
    );

    // Assert
    assert_eq!(
        result.expect("full-text predicate over table rows").rows(),
        &[vec![Value::Int64(1)]]
    );
}
