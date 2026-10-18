//! PostgreSQL 18 relation-source and join grammar contracts tracked by #800.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

fn join_fixture(label: &str) -> crate::support_sql_fixture::SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE issue_800_employees (id BIGINT, dept_id BIGINT, name TEXT)",
            "CREATE TABLE issue_800_departments (dept_id BIGINT, dept_name TEXT)",
            "INSERT INTO issue_800_employees VALUES (1, 10, 'Ada'), (2, 20, 'Lin')",
            "INSERT INTO issue_800_departments VALUES (10, 'Research'), (30, 'Design')",
        ],
    )
}

#[test]
fn should_merge_shared_columns_for_natural_inner_joins() {
    // Arrange
    let fixture = join_fixture("issue-800-natural-inner-join");

    // Act
    let result = fixture.execute(
        "SELECT * FROM issue_800_employees NATURAL JOIN issue_800_departments ORDER BY id",
    );

    // Assert
    let result = result.expect("NATURAL INNER JOIN");
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["dept_id", "id", "name", "dept_name"]
    );
    assert_eq!(
        result.rows(),
        &[vec![
            Value::Int64(10),
            Value::Int64(1),
            Value::String("Ada".to_owned()),
            Value::String("Research".to_owned()),
        ]]
    );
}

#[test]
fn should_preserve_unmatched_left_rows_for_natural_left_joins() {
    // Arrange
    let fixture = join_fixture("issue-800-natural-left-join");

    // Act
    let result = fixture.execute(
        "SELECT * FROM issue_800_employees NATURAL LEFT JOIN issue_800_departments ORDER BY id",
    );

    // Assert
    assert_eq!(
        result.expect("NATURAL LEFT JOIN").rows(),
        &[
            vec![
                Value::Int64(10),
                Value::Int64(1),
                Value::String("Ada".to_owned()),
                Value::String("Research".to_owned()),
            ],
            vec![
                Value::Int64(20),
                Value::Int64(2),
                Value::String("Lin".to_owned()),
                Value::Null,
            ],
        ]
    );
}

#[test]
fn should_preserve_unmatched_right_rows_for_natural_right_joins() {
    // Arrange
    let fixture = join_fixture("issue-800-natural-right-join");

    // Act
    let result = fixture.execute(
        "SELECT * FROM issue_800_employees NATURAL RIGHT JOIN issue_800_departments ORDER BY dept_id",
    );

    // Assert
    assert_eq!(
        result.expect("NATURAL RIGHT JOIN").rows(),
        &[
            vec![
                Value::Int64(10),
                Value::Int64(1),
                Value::String("Ada".to_owned()),
                Value::String("Research".to_owned()),
            ],
            vec![
                Value::Int64(30),
                Value::Null,
                Value::Null,
                Value::String("Design".to_owned()),
            ],
        ]
    );
}

#[test]
fn should_merge_both_sides_of_natural_full_joins() {
    // Arrange
    let fixture = join_fixture("issue-800-natural-full-join");

    // Act
    let result = fixture.execute(
        "SELECT * FROM issue_800_employees NATURAL FULL JOIN issue_800_departments ORDER BY dept_id",
    );

    // Assert
    assert_eq!(
        result.expect("NATURAL FULL JOIN").rows(),
        &[
            vec![
                Value::Int64(10),
                Value::Int64(1),
                Value::String("Ada".to_owned()),
                Value::String("Research".to_owned()),
            ],
            vec![
                Value::Int64(20),
                Value::Int64(2),
                Value::String("Lin".to_owned()),
                Value::Null,
            ],
            vec![
                Value::Int64(30),
                Value::Null,
                Value::Null,
                Value::String("Design".to_owned()),
            ],
        ]
    );
}

#[test]
fn should_merge_using_columns_once_in_select_star() {
    // Arrange
    let fixture = join_fixture("issue-800-join-using-star");

    // Act
    let result = fixture.execute(
        "SELECT * FROM issue_800_employees JOIN issue_800_departments USING (dept_id) ORDER BY id",
    );

    // Assert
    let result = result.expect("JOIN USING projection");
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["dept_id", "id", "name", "dept_name"]
    );
    assert_eq!(result.rows().len(), 1);
}

#[test]
fn should_apply_using_column_list_order_before_remaining_join_columns() {
    // Arrange
    let fixture = sql_fixture(
        "issue-800-join-using-column-order",
        &[
            "CREATE TABLE issue_800_left (id BIGINT, tenant BIGINT, left_value TEXT)",
            "CREATE TABLE issue_800_right (tenant BIGINT, id BIGINT, right_value TEXT)",
            "INSERT INTO issue_800_left VALUES (1, 9, 'left')",
            "INSERT INTO issue_800_right VALUES (9, 1, 'right')",
        ],
    );

    // Act
    let result =
        fixture.execute("SELECT * FROM issue_800_left JOIN issue_800_right USING (id, tenant)");

    // Assert
    let result = result.expect("multi-column JOIN USING");
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["id", "tenant", "left_value", "right_value"]
    );
    assert_eq!(result.rows().len(), 1);
}

#[test]
fn should_bind_the_using_alias_to_merged_join_columns() {
    // Arrange
    let fixture = join_fixture("issue-800-join-using-alias");

    // Act
    let result = fixture.execute(
        "SELECT merged.dept_id, issue_800_employees.name FROM issue_800_employees JOIN issue_800_departments USING (dept_id) AS merged ORDER BY merged.dept_id",
    );

    // Assert
    assert_eq!(
        result.expect("JOIN USING alias").rows(),
        &[vec![Value::Int64(10), Value::String("Ada".to_owned())]]
    );
}

#[test]
fn should_expand_qualified_stars_from_both_join_sides() {
    // Arrange
    let fixture = join_fixture("issue-800-qualified-join-stars");

    // Act
    let result = fixture.execute(
        "SELECT issue_800_employees.*, issue_800_departments.* FROM issue_800_employees JOIN issue_800_departments USING (dept_id)",
    );

    // Assert
    let result = result.expect("qualified stars across a join");
    assert_eq!(result.columns().len(), 5);
    assert_eq!(
        result.rows(),
        &[vec![
            Value::Int64(1),
            Value::Int64(10),
            Value::String("Ada".to_owned()),
            Value::Int64(10),
            Value::String("Research".to_owned()),
        ]]
    );
}

#[test]
fn should_expand_qualified_stars_from_derived_relations() {
    // Arrange
    let fixture = sql_fixture(
        "issue-800-qualified-derived-star",
        &[
            "CREATE TABLE issue_800_star_source (id BIGINT, label TEXT)",
            "INSERT INTO issue_800_star_source VALUES (1, 'one')",
        ],
    );

    // Act
    let result = fixture
        .execute("SELECT derived.* FROM (SELECT id, label FROM issue_800_star_source) AS derived");

    // Assert
    assert_eq!(
        result
            .expect("qualified star over a derived relation")
            .rows(),
        &[vec![Value::Int64(1), Value::String("one".to_owned())]]
    );
}

#[test]
fn should_expand_qualified_stars_from_common_table_expressions() {
    // Arrange
    let fixture = sql_fixture(
        "issue-800-qualified-cte-star",
        &[
            "CREATE TABLE issue_800_cte_star_source (id BIGINT, label TEXT)",
            "INSERT INTO issue_800_cte_star_source VALUES (1, 'one')",
        ],
    );

    // Act
    let result = fixture.execute(
        "WITH chosen AS (SELECT id, label FROM issue_800_cte_star_source) SELECT chosen.* FROM chosen",
    );

    // Assert
    assert_eq!(
        result.expect("qualified star over a CTE").rows(),
        &[vec![Value::Int64(1), Value::String("one".to_owned())]]
    );
}

#[test]
fn should_return_rows_from_a_zero_column_select_list() {
    // Arrange
    let fixture = sql_fixture(
        "issue-800-empty-select-list",
        &[
            "CREATE TABLE issue_800_empty_projection (id BIGINT)",
            "INSERT INTO issue_800_empty_projection VALUES (1), (2)",
        ],
    );

    // Act
    let result = fixture.execute("SELECT FROM issue_800_empty_projection ORDER BY id");

    // Assert
    let result = result.expect("zero-column SELECT list");
    assert_eq!(result.columns(), []);
    assert_eq!(result.rows(), &[vec![], vec![]]);
}

#[test]
fn should_select_a_table_as_a_query_source() {
    // Arrange
    let fixture = join_fixture("issue-800-table-query");

    // Act
    let result = fixture.execute("TABLE issue_800_employees ORDER BY id");

    // Assert
    assert_eq!(
        result.expect("TABLE query shorthand").rows(),
        &[
            vec![
                Value::Int64(1),
                Value::Int64(10),
                Value::String("Ada".to_owned()),
            ],
            vec![
                Value::Int64(2),
                Value::Int64(20),
                Value::String("Lin".to_owned()),
            ],
        ]
    );
}

#[test]
fn should_select_from_a_scalar_function_as_a_one_row_relation() {
    // Arrange
    let fixture = sql_fixture("issue-808-scalar-function-relation", &[]);

    // Act
    let result = fixture.execute("SELECT * FROM lower('X') AS function_row(value)");

    // Assert
    let result = result.expect("scalar function in FROM produces one row");
    assert_eq!(result.rows(), &[vec![Value::String("x".to_owned())]]);
    assert_eq!(result.columns()[0].type_oid, 25);
}

#[test]
fn should_expand_integer_generate_series_in_the_select_list() {
    // Arrange
    let fixture = sql_fixture("issue-808-generate-series-select-list", &[]);

    // Act
    let result = fixture.execute("SELECT generate_series(1, 5, 2) AS value");

    // Assert
    let result = result.expect("set-returning integer function in the select list");
    assert_eq!(
        result.rows(),
        &[
            vec![Value::Int64(1)],
            vec![Value::Int64(3)],
            vec![Value::Int64(5)],
        ]
    );
    assert_eq!(result.columns()[0].type_oid, 23);
}

#[test]
fn should_flatten_multidimensional_unnest_in_storage_order_with_ordinality() {
    // Arrange
    let fixture = sql_fixture("issue-808-multidimensional-unnest-ordinality", &[]);

    // Act
    let result = fixture.execute(
        "SELECT value, ordinal FROM unnest('[0:1][4:5]={{10,20},{30,40}}'::int4[]) WITH ORDINALITY AS expanded(value, ordinal) ORDER BY ordinal",
    );

    // Assert
    let result = result.expect("multidimensional UNNEST with ordinality");
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["value", "ordinal"]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [23, 20]
    );
    assert_eq!(
        result.rows(),
        &[
            vec![Value::Int64(10), Value::Int64(1)],
            vec![Value::Int64(20), Value::Int64(2)],
            vec![Value::Int64(30), Value::Int64(3)],
            vec![Value::Int64(40), Value::Int64(4)],
        ]
    );
}

#[test]
fn should_expand_rows_from_functions_in_lockstep() {
    // Arrange
    let fixture = sql_fixture("issue-808-rows-from-lockstep", &[]);

    // Act
    let result = fixture.execute(
        "SELECT * FROM ROWS FROM (unnest(ARRAY[10,20,30]), unnest(ARRAY['a','b'])) AS expanded(number,label)",
    );

    // Assert
    let result = result.expect("ROWS FROM combines function outputs by row position");
    assert_eq!(
        result.rows(),
        &[
            vec![Value::Int64(10), Value::String("a".to_owned())],
            vec![Value::Int64(20), Value::String("b".to_owned())],
            vec![Value::Int64(30), Value::Null],
        ]
    );
}

#[test]
fn should_accept_only_star_table_query_modifiers() {
    // Arrange
    let fixture = join_fixture("issue-800-table-only-star");

    // Act
    let only = fixture.execute("TABLE ONLY issue_800_employees ORDER BY id");
    let star = fixture.execute("TABLE issue_800_employees* ORDER BY id");

    // Assert
    let expected = vec![
        vec![
            Value::Int64(1),
            Value::Int64(10),
            Value::String("Ada".to_owned()),
        ],
        vec![
            Value::Int64(2),
            Value::Int64(20),
            Value::String("Lin".to_owned()),
        ],
    ];
    assert_eq!(only.expect("TABLE ONLY shorthand").rows(), expected);
    assert_eq!(star.expect("TABLE star shorthand").rows(), expected);
}

#[test]
fn should_use_table_queries_in_set_operation_branches() {
    // Arrange
    let fixture = sql_fixture(
        "issue-800-table-set-branch",
        &[
            "CREATE TABLE issue_800_table_left (id BIGINT)",
            "CREATE TABLE issue_800_table_right (id BIGINT)",
            "INSERT INTO issue_800_table_left VALUES (1)",
            "INSERT INTO issue_800_table_right VALUES (2)",
        ],
    );

    // Act
    let result = fixture
        .execute("TABLE issue_800_table_left UNION ALL TABLE issue_800_table_right ORDER BY id");

    // Assert
    assert_eq!(
        result.expect("TABLE in set-operation branches").rows(),
        &[vec![Value::Int64(1)], vec![Value::Int64(2)]]
    );
}

#[test]
fn should_use_table_queries_as_cte_bodies() {
    // Arrange
    let fixture = sql_fixture(
        "issue-800-table-cte-body",
        &[
            "CREATE TABLE issue_800_table_cte_source (id BIGINT)",
            "INSERT INTO issue_800_table_cte_source VALUES (4), (5)",
        ],
    );

    // Act
    let result = fixture.execute(
        "WITH chosen AS (TABLE issue_800_table_cte_source) SELECT * FROM chosen ORDER BY id",
    );

    // Assert
    assert_eq!(
        result.expect("TABLE CTE body").rows(),
        &[vec![Value::Int64(4)], vec![Value::Int64(5)]]
    );
}

#[test]
fn should_use_table_queries_as_derived_relations() {
    // Arrange
    let fixture = sql_fixture(
        "issue-800-table-derived-source",
        &[
            "CREATE TABLE issue_800_table_derived_source (id BIGINT)",
            "INSERT INTO issue_800_table_derived_source VALUES (6)",
        ],
    );

    // Act
    let result =
        fixture.execute("SELECT source.* FROM (TABLE issue_800_table_derived_source) AS source");

    // Assert
    assert_eq!(
        result.expect("TABLE derived relation").rows(),
        &[vec![Value::Int64(6)]]
    );
}

#[test]
fn should_insert_rows_from_a_table_query_source() {
    // Arrange
    let fixture = sql_fixture(
        "issue-800-insert-table-source",
        &[
            "CREATE TABLE issue_800_insert_source (id BIGINT)",
            "CREATE TABLE issue_800_insert_target (id BIGINT)",
            "INSERT INTO issue_800_insert_source VALUES (8), (9)",
        ],
    );

    // Act
    let inserted =
        fixture.execute("INSERT INTO issue_800_insert_target TABLE issue_800_insert_source");
    let selected = fixture.execute("SELECT id FROM issue_800_insert_target ORDER BY id");

    // Assert
    inserted.expect("INSERT TABLE source");
    assert_eq!(
        selected.expect("rows copied from TABLE source").rows(),
        &[vec![Value::Int64(8)], vec![Value::Int64(9)]]
    );
}

#[test]
fn should_update_inheritance_scans_when_child_attachment_changes() {
    // Arrange
    let fixture = sql_fixture(
        "issue-909-alter-inherit-scan",
        &[
            "CREATE TABLE issue_909_parent (id INT, label TEXT)",
            "CREATE TABLE issue_909_child (id INT, label TEXT, note TEXT)",
            "INSERT INTO issue_909_parent VALUES (1, 'parent')",
            "INSERT INTO issue_909_child VALUES (2, 'child', 'retained')",
        ],
    );

    // Act
    let attached = fixture.execute("ALTER TABLE issue_909_child INHERIT issue_909_parent");
    let inherited_rows = fixture.execute("SELECT id FROM issue_909_parent ORDER BY id");
    let detached = fixture.execute("ALTER TABLE issue_909_child NO INHERIT issue_909_parent");
    let parent_rows_after_detach = fixture.execute("SELECT id FROM issue_909_parent ORDER BY id");
    let child_rows_after_detach = fixture.execute("SELECT id FROM issue_909_child ORDER BY id");

    // Assert
    attached.expect("attach compatible child to parent");
    assert_eq!(
        inherited_rows
            .expect("scan attached inheritance tree")
            .rows(),
        &[vec![Value::Int64(1)], vec![Value::Int64(2)]]
    );
    detached.expect("detach child from parent");
    assert_eq!(
        parent_rows_after_detach
            .expect("scan parent after detaching child")
            .rows(),
        &[vec![Value::Int64(1)]]
    );
    assert_eq!(
        child_rows_after_detach
            .expect("child row survives inheritance detachment")
            .rows(),
        &[vec![Value::Int64(2)]]
    );
}

#[test]
fn should_mutate_inherited_rows_but_exclude_children_with_only() {
    // Arrange
    let fixture = sql_fixture(
        "issue-909-inherited-update-only",
        &[
            "CREATE TABLE issue_909_update_parent (id INT, amount INT)",
            "CREATE TABLE issue_909_update_child (id INT, amount INT, note TEXT)",
            "INSERT INTO issue_909_update_parent VALUES (1, 10)",
            "INSERT INTO issue_909_update_child VALUES (2, 20, 'child')",
            "ALTER TABLE issue_909_update_child INHERIT issue_909_update_parent",
        ],
    );

    // Act
    let inherited_update = fixture.execute(
        "UPDATE issue_909_update_parent SET amount = amount + 1 WHERE id = 2 RETURNING id, amount",
    );
    let only_parent_update = fixture.execute(
        "UPDATE ONLY issue_909_update_parent SET amount = amount + 10 WHERE id = 2 RETURNING id, amount",
    );
    let only_parent_rows =
        fixture.execute("SELECT id, amount FROM ONLY issue_909_update_parent ORDER BY id");
    let inherited_rows =
        fixture.execute("SELECT id, amount FROM issue_909_update_parent ORDER BY id");

    // Assert
    assert_eq!(
        inherited_update
            .expect("UPDATE parent includes matching descendant rows")
            .rows(),
        &[vec![Value::Int64(2), Value::Int64(21)]]
    );
    assert_eq!(
        only_parent_update
            .expect("UPDATE ONLY parent excludes descendant rows")
            .rows(),
        &[] as &[Vec<Value>]
    );
    assert_eq!(
        only_parent_rows
            .expect("scan only the parent relation")
            .rows(),
        &[vec![Value::Int64(1), Value::Int64(10)]]
    );
    assert_eq!(
        inherited_rows
            .expect("scan parent and inherited child rows")
            .rows(),
        &[
            vec![Value::Int64(1), Value::Int64(10)],
            vec![Value::Int64(2), Value::Int64(21)],
        ]
    );
}

#[test]
fn should_merge_shared_columns_from_multiple_inheritance_parents() {
    // Arrange
    let fixture = sql_fixture(
        "issue-909-multiple-inheritance-parents",
        &[
            "CREATE TABLE issue_909_parent_a (a INT, shared TEXT)",
            "CREATE TABLE issue_909_parent_b (b INT, shared TEXT)",
            "CREATE TABLE issue_909_multi_child (local_value INT) \
             INHERITS (issue_909_parent_a, issue_909_parent_b)",
            "INSERT INTO issue_909_multi_child VALUES (1, 'x', 2, 3)",
        ],
    );

    // Act
    let first_parent_rows = fixture.execute("SELECT a, shared FROM issue_909_parent_a ORDER BY a");
    let second_parent_rows = fixture.execute("SELECT b, shared FROM issue_909_parent_b ORDER BY b");
    let child = fixture.execute("SELECT * FROM issue_909_multi_child");

    // Assert
    assert_eq!(
        first_parent_rows
            .expect("scan the child through its first parent")
            .rows(),
        &[vec![Value::Int64(1), Value::String("x".to_owned())]]
    );
    assert_eq!(
        second_parent_rows
            .expect("scan the child through its second parent")
            .rows(),
        &[vec![Value::Int64(2), Value::String("x".to_owned())]]
    );
    let child = child.expect("scan a table inheriting from both parents");
    assert_eq!(
        child
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["a", "shared", "b", "local_value"]
    );
    assert_eq!(
        child.rows(),
        &[vec![
            Value::Int64(1),
            Value::String("x".to_owned()),
            Value::Int64(2),
            Value::Int64(3),
        ]]
    );
}

#[test]
fn should_propagate_new_parent_columns_to_existing_children() {
    // Arrange
    let fixture = sql_fixture(
        "issue-909-inherited-add-column",
        &["CREATE TABLE issue_909_alter_parent (id INT)"],
    );

    // Act
    let child = fixture.execute(
        "CREATE TABLE issue_909_alter_child (child_value TEXT) INHERITS (issue_909_alter_parent)",
    );
    child.expect("create a child inheriting the parent columns");
    fixture
        .execute("INSERT INTO issue_909_alter_child (id, child_value) VALUES (1, 'retained')")
        .expect("seed the child before the parent schema change");
    let altered = fixture
        .execute("ALTER TABLE issue_909_alter_parent ADD COLUMN inherited_value INT DEFAULT 7");
    let result = fixture
        .execute("SELECT id, child_value, inherited_value FROM issue_909_alter_child ORDER BY id");

    // Assert
    altered.expect("propagate a parent column change to existing descendants");
    let result = result.expect("read the altered child descriptor and row");
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.name.as_str())
            .collect::<Vec<_>>(),
        ["id", "child_value", "inherited_value"]
    );
    assert_eq!(
        result.rows(),
        &[vec![
            Value::String("retained".to_owned()),
            Value::Int64(1),
            Value::Int64(7),
        ]]
    );
}
