//! Direct inherited INSERT and DELETE semantics tracked by #909.

use cassie::types::Value;

use super::super::support_sql_fixture::sql_fixture;

#[test]
fn should_preserve_parent_insert_scope_when_deleting_descendants() {
    // Arrange
    let fixture = sql_fixture(
        "issue-909-inherited-insert-delete",
        &[
            "CREATE TABLE issue_909_dml_parent (id INT, label TEXT)",
            "CREATE TABLE issue_909_dml_child (extra INT) \
             INHERITS (issue_909_dml_parent)",
            "INSERT INTO issue_909_dml_child VALUES (1, 'child', 9)",
            "INSERT INTO issue_909_dml_parent VALUES (2, 'parent')",
        ],
    );

    // Act
    let inserted_parent = fixture
        .execute("INSERT INTO issue_909_dml_parent VALUES (3, 'inserted') RETURNING id, label");
    let parent_rows_before_delete =
        fixture.execute("SELECT id, label FROM ONLY issue_909_dml_parent ORDER BY id");
    let child_rows_before_delete =
        fixture.execute("SELECT id, label FROM issue_909_dml_child ORDER BY id");
    let only_delete =
        fixture.execute("DELETE FROM ONLY issue_909_dml_parent WHERE id = 1 RETURNING id, label");
    let child_rows_after_only_delete =
        fixture.execute("SELECT id, label FROM issue_909_dml_child ORDER BY id");
    let inherited_delete =
        fixture.execute("DELETE FROM issue_909_dml_parent WHERE id = 1 RETURNING id, label");
    let parent_delete =
        fixture.execute("DELETE FROM ONLY issue_909_dml_parent WHERE id = 2 RETURNING id, label");
    let parent_rows_after_delete =
        fixture.execute("SELECT id, label FROM issue_909_dml_parent ORDER BY id");
    let child_rows_after_delete =
        fixture.execute("SELECT id, label FROM issue_909_dml_child ORDER BY id");

    // Assert
    assert_eq!(
        inserted_parent
            .expect("insert a row into the named parent")
            .rows(),
        &[vec![Value::Int64(3), Value::String("inserted".to_owned())]]
    );
    assert_eq!(
        parent_rows_before_delete
            .expect("read only the named parent after INSERT")
            .rows(),
        &[
            vec![Value::Int64(2), Value::String("parent".to_owned())],
            vec![Value::Int64(3), Value::String("inserted".to_owned())],
        ]
    );
    assert_eq!(
        child_rows_before_delete
            .expect("read child rows independently of parent INSERT")
            .rows(),
        &[vec![Value::Int64(1), Value::String("child".to_owned())]]
    );
    assert_eq!(
        only_delete
            .expect("DELETE ONLY must not target a descendant row")
            .rows(),
        &[] as &[Vec<Value>]
    );
    assert_eq!(
        child_rows_after_only_delete
            .expect("child row remains after DELETE ONLY")
            .rows(),
        &[vec![Value::Int64(1), Value::String("child".to_owned())]]
    );
    assert_eq!(
        inherited_delete
            .expect("DELETE from the parent includes matching descendants")
            .rows(),
        &[vec![Value::Int64(1), Value::String("child".to_owned())]]
    );
    assert_eq!(
        parent_delete
            .expect("DELETE ONLY targets the matching parent row")
            .rows(),
        &[vec![Value::Int64(2), Value::String("parent".to_owned())]]
    );
    assert_eq!(
        parent_rows_after_delete
            .expect("read parent and descendants after deletes")
            .rows(),
        &[vec![Value::Int64(3), Value::String("inserted".to_owned())]]
    );
    assert_eq!(
        child_rows_after_delete
            .expect("read child after inherited DELETE")
            .rows(),
        &[] as &[Vec<Value>]
    );
}
