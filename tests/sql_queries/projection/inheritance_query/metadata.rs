#[test]
fn should_keep_parent_projection_descriptor_when_children_add_columns() {
    // Arrange
    let fixture = sql_fixture(
        "issue-909-parent-projection-descriptor",
        &[
            "CREATE TABLE issue_909_projection_parent (id INT, label TEXT)",
            "CREATE TABLE issue_909_projection_child (amount NUMERIC(8,2), child_only TEXT) \
             INHERITS (issue_909_projection_parent)",
            "INSERT INTO issue_909_projection_parent VALUES (2, 'parent')",
            "INSERT INTO issue_909_projection_child (id, label, amount, child_only) \
             VALUES (1, 'child', 2.50, 'extra')",
        ],
    );

    // Act
    let result = fixture.execute("SELECT * FROM issue_909_projection_parent ORDER BY id");

    // Assert
    let result = result.expect("parent SELECT * scans descendants using the parent row shape");
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["id", "label"]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [23, 25]
    );
    assert_eq!(
        result.rows(),
        &[
            vec![Value::Int64(1), Value::String("child".to_owned())],
            vec![Value::Int64(2), Value::String("parent".to_owned())],
        ]
    );
}
