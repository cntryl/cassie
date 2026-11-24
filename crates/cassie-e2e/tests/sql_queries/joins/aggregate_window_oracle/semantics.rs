#[test]
fn should_project_composite_primary_key_dependencies_through_a_join() {
    // Arrange
    let fixture = sql_fixture("issue-761-composite-primary-key-dependency-join", &[]);
    fixture
        .execute("CREATE TABLE issue_761_fd_parent (tenant_id INT, item_id INT, label TEXT, CONSTRAINT issue_761_fd_parent_custom_key PRIMARY KEY (tenant_id, item_id))")
        .expect("create composite-primary-key parent");
    fixture
        .execute("CREATE TABLE issue_761_fd_child (tenant_id INT, item_id INT, amount INT)")
        .expect("create child");
    fixture
        .execute("INSERT INTO issue_761_fd_parent VALUES (1,1,'alpha'),(1,2,'beta'),(2,1,'gamma')")
        .expect("seed parent");
    fixture
        .execute("INSERT INTO issue_761_fd_child VALUES (1,1,2),(1,1,3),(1,2,7),(2,1,11)")
        .expect("seed child");
    assert!(fixture
        .execute("INSERT INTO issue_761_fd_parent VALUES (1,1,'duplicate')")
        .is_err());
    fixture
        .execute("INSERT INTO issue_761_fd_parent VALUES (1,1,'ignored') ON CONFLICT ON CONSTRAINT issue_761_fd_parent_custom_key DO NOTHING")
        .expect("custom primary-key constraint arbitrates tuple conflict");
    let key_fields = fixture
        .cassie
        .catalog
        .list_indexes("issue_761_fd_parent")
        .into_iter()
        .find(|index| index.name == "issue_761_fd_parent_custom_key")
        .expect("custom primary-key index")
        .normalized_fields();
    let parent_count = fixture.rows("SELECT COUNT(*) FROM issue_761_fd_parent");
    assert_eq!(key_fields, vec!["tenant_id", "item_id"]);
    assert_eq!(parent_count, vec![vec![Value::Int64(3)]]);

    // Act
    let result = fixture.execute(
        "SELECT parent.tenant_id, parent.item_id, parent.label, SUM(child.amount), COUNT(*) \
         FROM issue_761_fd_parent AS parent \
         JOIN issue_761_fd_child AS child \
           ON parent.tenant_id = child.tenant_id AND parent.item_id = child.item_id \
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
