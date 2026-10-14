//! TRUNCATE ONLY inheritance semantics tracked by #823.

use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_truncate_only_the_named_inherited_relation() {
    // Arrange
    let fixture = sql_fixture(
        "issue-823-truncate-only-inheritance",
        &[
            "CREATE TABLE issue_823_truncate_only_parent (id INT)",
            "CREATE TABLE issue_823_truncate_only_child (extra TEXT) \
             INHERITS (issue_823_truncate_only_parent)",
            "INSERT INTO issue_823_truncate_only_parent VALUES (1)",
            "INSERT INTO issue_823_truncate_only_child (id, extra) VALUES (2, 'child')",
        ],
    );

    // Act
    let truncated = fixture.execute("TRUNCATE TABLE ONLY issue_823_truncate_only_parent");
    let parent_rows =
        fixture.execute("SELECT id FROM ONLY issue_823_truncate_only_parent ORDER BY id");
    let child_rows =
        fixture.execute("SELECT id, extra FROM issue_823_truncate_only_child ORDER BY id");
    let inherited_rows =
        fixture.execute("SELECT id FROM issue_823_truncate_only_parent ORDER BY id");

    // Assert
    truncated.expect("TRUNCATE ONLY accepts an inherited parent relation");
    assert_eq!(
        parent_rows
            .expect("read only the named parent after TRUNCATE ONLY")
            .rows(),
        &[] as &[Vec<Value>]
    );
    assert_eq!(
        child_rows
            .expect("the explicitly inherited child remains intact")
            .rows(),
        &[vec![Value::Int64(2), Value::String("child".to_owned())]]
    );
    assert_eq!(
        inherited_rows
            .expect("parent scans continue to include surviving descendants")
            .rows(),
        &[vec![Value::Int64(2)]]
    );
}
