#[test]
fn should_project_composite_primary_key_dependencies_through_a_join() {
    // Arrange
    let fixture = sql_fixture(
        "issue-804-composite-primary-key-dependency-join",
        &[
            "CREATE TABLE issue_804_fd_parent (tenant_id INT, item_id INT, label TEXT, PRIMARY KEY (tenant_id, item_id))",
            "CREATE TABLE issue_804_fd_child (tenant_id INT, item_id INT, amount INT)",
            "INSERT INTO issue_804_fd_parent VALUES (1,1,'alpha'),(1,2,'beta'),(2,1,'gamma')",
            "INSERT INTO issue_804_fd_child VALUES (1,1,2),(1,1,3),(1,2,7),(2,1,11)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT parent.tenant_id, parent.item_id, parent.label, SUM(child.amount), COUNT(*) \
         FROM issue_804_fd_parent AS parent \
         JOIN issue_804_fd_child AS child USING (tenant_id, item_id) \
         GROUP BY parent.tenant_id, parent.item_id \
         ORDER BY parent.tenant_id, parent.item_id",
    );

    // Assert
    let result = result.expect("the composite primary key determines the projected label");
    assert_eq!(
        result.rows(),
        &[
            vec![
                Value::Int64(1),
                Value::Int64(1),
                Value::String("alpha".to_owned()),
                Value::Int64(5),
                Value::Int64(2),
            ],
            vec![
                Value::Int64(1),
                Value::Int64(2),
                Value::String("beta".to_owned()),
                Value::Int64(7),
                Value::Int64(1),
            ],
            vec![
                Value::Int64(2),
                Value::Int64(1),
                Value::String("gamma".to_owned()),
                Value::Int64(11),
                Value::Int64(1),
            ],
        ]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 23, 25, 20, 20]
    );
}
