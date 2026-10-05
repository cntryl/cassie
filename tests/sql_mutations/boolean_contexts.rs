use super::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::app::CassieError;
use cassie::types::Value;

fn mutation_fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE bool_writes (id INT PRIMARY KEY, n INT, txt TEXT, flag BOOLEAN)",
            "INSERT INTO bool_writes (id, n, txt, flag) VALUES (1, 10, 'no', FALSE)",
            "CREATE TABLE bool_empty_writes (id INT PRIMARY KEY, n INT, txt TEXT, flag BOOLEAN)",
        ],
    )
}

#[test]
fn should_reject_non_boolean_mutation_predicates_before_publication() {
    // Arrange
    let fixture = mutation_fixture("bool-write-predicate-types");
    let statements = [
        "UPDATE bool_writes SET n = 99 WHERE 1",
        "UPDATE bool_writes SET n = 99 WHERE txt",
        "UPDATE bool_writes SET n = 99 WHERE TRUE OR n",
        "DELETE FROM bool_writes WHERE n",
        "DELETE FROM bool_writes WHERE NOT txt",
        "DELETE FROM bool_writes WHERE FALSE AND 1",
    ];

    for sql in statements {
        // Act
        let result = fixture.execute(sql);
        let retained = fixture.rows("SELECT id, n, txt, flag FROM bool_writes ORDER BY id");

        // Assert
        assert!(
            matches!(result, Err(CassieError::Planner(_))),
            "static predicate: {sql}: {result:?}"
        );
        assert_eq!(
            retained,
            vec![vec![
                Value::Int64(1),
                Value::Int64(10),
                Value::String("no".to_string()),
                Value::Bool(false)
            ]]
        );
    }
}

#[test]
fn should_validate_mutation_boolean_subexpressions_before_row_evaluation() {
    // Arrange
    let fixture = mutation_fixture("bool-empty-write-types");
    let statements = [
        "UPDATE bool_empty_writes SET n = 99 WHERE n",
        "DELETE FROM bool_empty_writes WHERE txt",
        "UPDATE bool_empty_writes SET flag = 1 AND TRUE WHERE TRUE",
        "UPDATE bool_empty_writes SET n = 99 WHERE TRUE RETURNING NOT txt",
        "DELETE FROM bool_empty_writes WHERE TRUE RETURNING n OR FALSE",
        "INSERT INTO bool_writes (id, n, txt, flag) VALUES (2, 20, 'no', CAST(NOT 1 AS BOOLEAN))",
    ];

    // Act
    let results = statements.map(|sql| fixture.execute(sql));

    // Assert
    for (sql, result) in statements.into_iter().zip(results) {
        assert!(
            matches!(result, Err(CassieError::Planner(_))),
            "validate expressions before evaluating rows: {sql}: {result:?}"
        );
    }
    assert_eq!(
        fixture.rows("SELECT id, n FROM bool_writes ORDER BY id"),
        vec![vec![Value::Int64(1), Value::Int64(10)]]
    );
    assert_eq!(
        fixture.rows("SELECT id FROM bool_empty_writes"),
        Vec::<Vec<Value>>::new()
    );
}

#[test]
fn should_reject_typed_conflict_update_predicates_without_modifying_the_existing_row() {
    // Arrange
    let fixture = mutation_fixture("bool-conflict-predicate-types");
    let statements = [
        "INSERT INTO bool_writes (id, n, txt, flag) VALUES (1, 99, 'changed', TRUE) ON CONFLICT (id) DO UPDATE SET n = excluded.n WHERE 1 RETURNING n",
        "INSERT INTO bool_writes (id, n, txt, flag) VALUES (1, 99, 'changed', TRUE) ON CONFLICT (id) DO UPDATE SET n = excluded.n WHERE excluded.txt RETURNING n",
        "INSERT INTO bool_writes (id, n, txt, flag) VALUES (1, 99, 'changed', TRUE) ON CONFLICT (id) DO UPDATE SET n = excluded.n WHERE TRUE OR excluded.n RETURNING n",
    ];

    for sql in statements {
        // Act
        let result = fixture.execute(sql);
        let retained = fixture.rows("SELECT n, flag FROM bool_writes WHERE id = 1");

        // Assert
        assert!(
            matches!(result, Err(CassieError::Planner(_))),
            "conflict Boolean context: {sql}: {result:?}"
        );
        assert_eq!(retained, vec![vec![Value::Int64(10), Value::Bool(false)]]);
    }
}

#[test]
fn should_select_no_mutation_rows_for_false_unknown_predicates() {
    // Arrange
    let fixture = mutation_fixture("bool-write-unknown-no");
    let statements = [
        "UPDATE bool_writes SET n = 99 WHERE 'no' RETURNING n",
        "DELETE FROM bool_writes WHERE 'no' RETURNING n",
        "INSERT INTO bool_writes (id, n, txt, flag) VALUES (1, 99, 'changed', TRUE) ON CONFLICT (id) DO UPDATE SET n = excluded.n WHERE 'no' RETURNING n",
    ];

    for sql in statements {
        // Act
        let result = fixture
            .execute(sql)
            .expect("unknown no is a Boolean false predicate");
        let retained = fixture.rows("SELECT n, flag FROM bool_writes WHERE id = 1");

        // Assert
        assert_eq!(
            result.rows,
            Vec::<Vec<Value>>::new(),
            "false predicates produce no RETURNING rows: {sql}"
        );
        assert_eq!(retained, vec![vec![Value::Int64(10), Value::Bool(false)]]);
    }
}

#[test]
fn should_preserve_canonical_boolean_assignment_values_through_returning() {
    // Arrange
    for column_store in [false, true] {
        let suffix = if column_store {
            " WITH (storage = column_store)"
        } else {
            ""
        };
        let create = format!("CREATE TABLE bool_canonical_writes (id INT, flag BOOLEAN){suffix}");
        let fixture = sql_fixture("bool-canonical-write-inputs", &[create.as_str()]);

        // Act
        let inserted = fixture
            .execute("INSERT INTO bool_canonical_writes (id, flag) VALUES (1, 'no') RETURNING flag")
            .expect("unknown Boolean input at declared assignment");
        let updated = fixture.execute("UPDATE bool_canonical_writes SET flag = 'yes' WHERE id = 1 RETURNING flag, NOT flag")
            .expect("canonical Boolean assignment and RETURNING expression");
        let stored = fixture.rows("SELECT flag FROM bool_canonical_writes WHERE id = 1");

        // Assert
        assert_eq!(inserted.rows, vec![vec![Value::Bool(false)]]);
        assert_eq!(inserted.columns[0].type_oid, 16);
        assert_eq!(
            updated.rows,
            vec![vec![Value::Bool(true), Value::Bool(false)]]
        );
        assert!(updated.columns.iter().all(|column| column.type_oid == 16));
        assert_eq!(stored, vec![vec![Value::Bool(true)]]);
    }
}

#[test]
fn should_reject_invalid_boolean_input_before_empty_mutations_or_insert_publication() {
    // Arrange
    let fixture = mutation_fixture("bool-write-invalid-input");
    let statements = [
        "UPDATE bool_empty_writes SET n = 99 WHERE 'o'",
        "DELETE FROM bool_empty_writes WHERE 'nonempty'",
        "INSERT INTO bool_writes (id, n, txt, flag) VALUES (2, 20, 'second', 'o') RETURNING flag",
        "INSERT INTO bool_writes (id, n, txt, flag) VALUES (1, 99, 'changed', TRUE) ON CONFLICT (id) DO UPDATE SET n = excluded.n WHERE 'o' RETURNING n",
    ];

    // Act
    let results = statements.map(|sql| fixture.execute(sql));

    // Assert
    for (sql, result) in statements.into_iter().zip(results) {
        assert!(result.is_err(), "invalid Boolean input: {sql}: {result:?}");
    }
    assert_eq!(
        fixture.rows("SELECT id, n FROM bool_writes ORDER BY id"),
        vec![vec![Value::Int64(1), Value::Int64(10)]]
    );
}

#[test]
fn should_use_declared_types_for_conflict_boolean_aliases() {
    // Arrange
    let fixture = mutation_fixture("bool-conflict-alias-types");

    // Act
    let typed_text = fixture.execute("INSERT INTO bool_writes (id, n, txt, flag) VALUES (1, 99, 'no', TRUE) ON CONFLICT (id) DO UPDATE SET n = excluded.n WHERE excluded.txt RETURNING n");
    let after_text = fixture.rows("SELECT n, flag FROM bool_writes WHERE id = 1");
    let false_boolean = fixture.execute("INSERT INTO bool_writes (id, n, txt, flag) VALUES (1, 88, 'no', FALSE) ON CONFLICT (id) DO UPDATE SET n = excluded.n WHERE excluded.flag RETURNING n");
    let after_false = fixture.rows("SELECT n, flag FROM bool_writes WHERE id = 1");
    let true_boolean = fixture.execute("INSERT INTO bool_writes (id, n, txt, flag) VALUES (1, 77, 'no', TRUE) ON CONFLICT (id) DO UPDATE SET n = excluded.n WHERE excluded.flag RETURNING n");

    // Assert
    assert!(matches!(typed_text, Err(CassieError::Planner(_))));
    assert_eq!(after_text, vec![vec![Value::Int64(10), Value::Bool(false)]]);
    assert_eq!(
        false_boolean.expect("declared excluded Boolean FALSE").rows,
        Vec::<Vec<Value>>::new()
    );
    assert_eq!(
        after_false,
        vec![vec![Value::Int64(10), Value::Bool(false)]]
    );
    assert_eq!(
        true_boolean.expect("declared excluded Boolean TRUE").rows,
        vec![vec![Value::Int64(77)]]
    );
}

#[test]
fn should_contextualize_unknown_boolean_insert_select_assignments() {
    // Arrange
    for column_store in [false, true] {
        let suffix = if column_store {
            " WITH (storage = column_store)"
        } else {
            ""
        };
        let create = format!("CREATE TABLE bool_select_target (row_key INT, flag BOOLEAN){suffix}");
        let fixture = sql_fixture("bool-insert-select-unknown", &[create.as_str()]);

        // Act
        let control = fixture.execute(
            "INSERT INTO bool_select_target (row_key, flag) SELECT 1, FALSE RETURNING flag",
        );
        let contextual = fixture.execute(
            "INSERT INTO bool_select_target (row_key, flag) SELECT 2, 'no' RETURNING flag",
        );
        let stored = fixture.rows("SELECT row_key, flag FROM bool_select_target ORDER BY row_key");

        // Assert
        let control = control.expect("supported INSERT SELECT Boolean control");
        let contextual =
            contextual.expect("direct unknown SQL literal uses destination Boolean context");
        assert_eq!(control.rows, vec![vec![Value::Bool(false)]]);
        assert_eq!(contextual.rows, vec![vec![Value::Bool(false)]]);
        assert_eq!(control.columns[0].type_oid, 16);
        assert_eq!(contextual.columns[0].type_oid, 16);
        assert_eq!(
            stored,
            vec![
                vec![Value::Int64(1), Value::Bool(false)],
                vec![Value::Int64(2), Value::Bool(false)]
            ]
        );
    }
}

#[test]
fn should_reject_typed_boolean_insert_select_assignments_before_empty_execution() {
    // Arrange
    for column_store in [false, true] {
        let suffix = if column_store {
            " WITH (storage = column_store)"
        } else {
            ""
        };
        let create = format!("CREATE TABLE bool_select_target (row_key INT, flag BOOLEAN){suffix}");
        let fixture = sql_fixture(
            "bool-insert-select-static",
            &[
                create.as_str(),
                "CREATE TABLE bool_select_empty (row_key INT, txt TEXT, flag BOOLEAN)",
            ],
        );
        let statements = [
            "INSERT INTO bool_select_target (row_key, flag) SELECT row_key, txt FROM bool_select_empty RETURNING flag",
            "INSERT INTO bool_select_target (row_key, flag) SELECT row_key, CAST('no' AS TEXT) FROM bool_select_empty RETURNING flag",
            "INSERT INTO bool_select_target (row_key, flag) SELECT row_key, 'o' FROM bool_select_empty RETURNING flag",
        ];

        // Act
        let control = fixture.execute("INSERT INTO bool_select_target (row_key, flag) SELECT row_key, flag FROM bool_select_empty RETURNING flag");
        let results = statements.map(|sql| fixture.execute(sql));
        let stored = fixture.rows("SELECT row_key FROM bool_select_target");

        // Assert
        assert_eq!(
            control
                .expect("typed Boolean projection is a supported empty source")
                .rows,
            Vec::<Vec<Value>>::new()
        );
        for (sql, result) in statements.into_iter().zip(results) {
            assert!(
                matches!(result, Err(CassieError::Planner(_))),
                "declared Boolean target validates before source execution: {sql}: {result:?}"
            );
        }
        assert_eq!(stored, Vec::<Vec<Value>>::new());
    }
}

#[test]
fn should_validate_typed_boolean_assignment_parameters_before_empty_mutations() {
    // Arrange
    let fixture = mutation_fixture("bool-write-parameter-types");
    let statements = [
        "UPDATE bool_empty_writes SET flag = $1 WHERE TRUE RETURNING flag",
        "INSERT INTO bool_empty_writes (id, flag) SELECT n, $1 FROM bool_empty_writes RETURNING flag",
    ];

    for sql in statements {
        // Act
        let boolean = fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![Value::Bool(false)]);
        let text = fixture.cassie.execute_sql(
            &fixture.session,
            sql,
            vec![Value::String("no".to_string())],
        );
        let numeric = fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![Value::Int64(0)]);

        // Assert
        assert_eq!(
            boolean
                .expect("typed Boolean assignment on empty input")
                .rows,
            Vec::<Vec<Value>>::new(),
            "{sql}"
        );
        assert!(
            matches!(text, Err(CassieError::Planner(_))),
            "typed TEXT assignment: {sql}: {text:?}"
        );
        assert!(
            matches!(numeric, Err(CassieError::Planner(_))),
            "typed numeric assignment: {sql}: {numeric:?}"
        );
    }
    assert_eq!(
        fixture.rows("SELECT id FROM bool_empty_writes"),
        Vec::<Vec<Value>>::new()
    );
}

#[test]
fn should_validate_window_parameter_types_at_boolean_insert_destinations() {
    // Arrange
    let fixture = sql_fixture(
        "bool-window-insert-parameter-types",
        &[
            "CREATE TABLE bool_window_target (flag BOOLEAN)",
            "CREATE TABLE bool_window_empty (n INT)",
        ],
    );
    let sql = "INSERT INTO bool_window_target (flag) SELECT first_value($1) OVER (ORDER BY n) FROM bool_window_empty RETURNING flag";

    // Act
    let boolean = fixture
        .cassie
        .execute_sql(&fixture.session, sql, vec![Value::Bool(false)]);
    let null = fixture
        .cassie
        .execute_sql(&fixture.session, sql, vec![Value::Null]);
    let text =
        fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![Value::String("no".to_string())]);
    let stored = fixture.rows("SELECT flag FROM bool_window_target");

    // Assert
    let boolean = boolean.expect("supported first_value INSERT SELECT with declared Boolean input");
    let null = null.expect("NULL remains assignable to a nullable Boolean target");
    assert_eq!(boolean.rows, Vec::<Vec<Value>>::new());
    assert_eq!(null.rows, Vec::<Vec<Value>>::new());
    assert_eq!(boolean.columns[0].type_oid, 16);
    assert_eq!(null.columns[0].type_oid, 16);
    assert!(
        matches!(text, Err(CassieError::Planner(_))),
        "typed TEXT window output must reject before the empty source: {text:?}"
    );
    assert_eq!(stored, Vec::<Vec<Value>>::new());
}

#[test]
fn should_validate_exported_wildcard_parameter_types_at_boolean_insert_destinations() {
    // Arrange
    let fixture = sql_fixture(
        "bool-wildcard-insert-parameter-types",
        &[
            "CREATE TABLE bool_wildcard_target (flag BOOLEAN)",
            "CREATE TABLE bool_wildcard_empty (n INT)",
        ],
    );
    let statements = [
        "INSERT INTO bool_wildcard_target (flag) SELECT * FROM (SELECT $1 AS flag FROM bool_wildcard_empty) AS d RETURNING flag",
    ];
    for sql in statements {
        // Act
        let boolean = fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![Value::Bool(false)]);
        let null = fixture
            .cassie
            .execute_sql(&fixture.session, sql, vec![Value::Null]);
        let text = fixture.cassie.execute_sql(
            &fixture.session,
            sql,
            vec![Value::String("no".to_string())],
        );

        // Assert
        assert_eq!(
            boolean.expect("supported wildcard Boolean source").rows,
            Vec::<Vec<Value>>::new(),
            "{sql}"
        );
        assert_eq!(
            null.expect("NULL wildcard output stays assignable").rows,
            Vec::<Vec<Value>>::new(),
            "{sql}"
        );
        assert!(
            matches!(text, Err(CassieError::Planner(_))),
            "typed TEXT wildcard output: {sql}: {text:?}"
        );
    }
    assert_eq!(
        fixture.rows("SELECT flag FROM bool_wildcard_target"),
        Vec::<Vec<Value>>::new()
    );
}
