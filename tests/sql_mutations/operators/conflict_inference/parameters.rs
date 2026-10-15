//! PostgreSQL 18 conflict-target inference gaps for #825.

use cassie::types::Value;

use super::support_sql_fixture::sql_fixture;

#[test]
fn should_infer_conflict_keys_for_operator_class_expression_index() {
    // Arrange
    let fixture = sql_fixture(
        "issue-825-conflict-inference-column-expression-opclass",
        &[
            "CREATE TABLE issue_825_conflict_opclass_expression (id INT PRIMARY KEY, email TEXT, token TEXT, payload TEXT)",
            "CREATE UNIQUE INDEX issue_825_conflict_opclass_expression_key \
             ON issue_825_conflict_opclass_expression (email, (lower(token)))",
            "INSERT INTO issue_825_conflict_opclass_expression \
             VALUES (1, 'a@example.com', 'Token', 'before')",
        ],
    );

    // Act
    let changed = fixture.execute(
        "INSERT INTO issue_825_conflict_opclass_expression \
         VALUES (2, 'a@example.com', 'token', 'after') \
         ON CONFLICT (email text_ops, (lower(token)) text_ops) \
         DO UPDATE SET payload = EXCLUDED.payload \
         RETURNING id, email, token, payload",
    );
    let stored = fixture.execute(
        "SELECT id, email, token, payload \
         FROM issue_825_conflict_opclass_expression ORDER BY id",
    );

    // Assert
    let changed = changed.expect("infer both the column and expression index elements");
    assert_eq!(
        changed.rows(),
        &[vec![
            Value::Int64(1),
            Value::String("a@example.com".to_owned()),
            Value::String("Token".to_owned()),
            Value::String("after".to_owned()),
        ]]
    );
    assert_eq!(
        changed
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 25, 25, 25]
    );
    assert_eq!(
        stored.expect("read the inferred conflict row").rows(),
        &[vec![
            Value::Int64(1),
            Value::String("a@example.com".to_owned()),
            Value::String("Token".to_owned()),
            Value::String("after".to_owned()),
        ]]
    );
}
