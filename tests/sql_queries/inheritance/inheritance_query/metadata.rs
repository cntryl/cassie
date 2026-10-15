#[test]
fn should_filter_an_inherited_parent_scan_by_child_tableoid() {
    // Arrange
    let fixture = sql_fixture(
        "issue-909-tableoid-child-filter",
        &[
            "CREATE TABLE issue_909_tableoid_parent (id INT, label TEXT)",
            "CREATE TABLE issue_909_tableoid_child (child_only TEXT) \
             INHERITS (issue_909_tableoid_parent)",
            "INSERT INTO issue_909_tableoid_parent VALUES (2, 'parent')",
            "INSERT INTO issue_909_tableoid_child (id, label, child_only) \
             VALUES (1, 'child', 'extra')",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT id, tableoid = 'issue_909_tableoid_child'::regclass AS is_child \
         FROM issue_909_tableoid_parent ORDER BY id",
    );

    // Assert
    let result = result.expect("tableoid identifies each physical relation in an inherited scan");
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["id", "is_child"]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [23, 16]
    );
    assert_eq!(
        result.rows(),
        &[
            vec![Value::Int64(1), Value::Bool(true)],
            vec![Value::Int64(2), Value::Bool(false)],
        ]
    );
}
