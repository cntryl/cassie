//! Per-cell defaults use the same allocator as omitted INSERT columns.
use super::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_apply_default_cells_given_reordered_columns_when_inserting_mixed_rows() {
    // Arrange
    let fixture = sql_fixture("insert-default-cells", &[
        "CREATE TABLE default_cells (id INT, label TEXT DEFAULT 'pending', version INT DEFAULT 7, optional TEXT)",
    ]);

    // Act
    let inserted = fixture.execute("INSERT INTO default_cells(version,id,optional,label) VALUES (DEFAULT,1,DEFAULT,'custom'),(9,2,NULL,DEFAULT) RETURNING id,label,version,optional");

    // Assert
    assert_eq!(
        inserted.expect("mixed DEFAULT cells").rows(),
        &[
            vec![
                Value::Int64(1),
                Value::String("custom".into()),
                Value::Int64(7),
                Value::Null
            ],
            vec![
                Value::Int64(2),
                Value::String("pending".into()),
                Value::Int64(9),
                Value::Null
            ],
        ]
    );
}

#[test]
fn should_preserve_explicit_json_sql_null_given_a_nonnull_column_default() {
    // Arrange
    let fixture = sql_fixture("insert-json-null-default", &[
        "CREATE TABLE json_null_default (id INT PRIMARY KEY, v JSONB DEFAULT '{\"k\":1}'::jsonb)",
    ]);

    // Act
    let inserted = fixture.execute("INSERT INTO json_null_default VALUES (1,NULL),(2,'null'::jsonb) RETURNING id,v IS NULL,v::text");

    // Assert
    assert_eq!(
        inserted.expect("SQL NULL must suppress the default").rows(),
        &[
            vec![Value::Int64(1), Value::Bool(true), Value::Null],
            vec![
                Value::Int64(2),
                Value::Bool(false),
                Value::String("null".into())
            ],
        ]
    );
}

#[test]
fn should_allocate_serial_defaults_once_given_explicit_default_cells() {
    // Arrange
    let fixture = sql_fixture(
        "insert-default-serial",
        &["CREATE TABLE serial_default_cells (id SERIAL PRIMARY KEY, label TEXT)"],
    );

    // Act
    let inserted = fixture.execute("INSERT INTO serial_default_cells VALUES (DEFAULT,'a'),(9,'explicit'),(DEFAULT,'b') RETURNING id");
    let next =
        fixture.execute("INSERT INTO serial_default_cells(label) VALUES ('next') RETURNING id");

    // Assert
    assert_eq!(
        inserted.expect("serial DEFAULT cells").rows(),
        &[
            vec![Value::Int64(1)],
            vec![Value::Int64(9)],
            vec![Value::Int64(2)],
        ]
    );
    assert_eq!(
        next.expect("next serial allocation").rows(),
        &[vec![Value::Int64(3)]]
    );
}

#[test]
fn should_apply_identity_override_given_default_cells_when_generating_always_values() {
    // Arrange
    let fixture = sql_fixture(
        "insert-default-always",
        &["CREATE TABLE always_default_cells (id INT GENERATED ALWAYS AS IDENTITY, label TEXT)"],
    );

    // Act
    let generated = fixture.execute(
        "INSERT INTO always_default_cells VALUES (DEFAULT,'a'),(DEFAULT,'b') RETURNING id",
    );
    let overridden = fixture.execute("INSERT INTO always_default_cells OVERRIDING USER VALUE VALUES (99,'ignored'),(DEFAULT,'generated') RETURNING id");

    // Assert
    assert_eq!(
        generated.expect("ALWAYS accepts DEFAULT").rows(),
        &[vec![Value::Int64(1)], vec![Value::Int64(2)]]
    );
    assert_eq!(
        overridden.expect("USER override allocates once").rows(),
        &[vec![Value::Int64(3)], vec![Value::Int64(4)]]
    );
}

#[test]
fn should_reject_explicit_always_identity_before_allocating_default_cells() {
    // Arrange
    let fixture = sql_fixture("insert-default-always-rejection", &[
        "CREATE TABLE always_default_rejected (id INT GENERATED ALWAYS AS IDENTITY, label TEXT)",
    ]);

    // Act
    let rejected = fixture.execute(
        "INSERT INTO always_default_rejected VALUES (DEFAULT,'generated'),(NULL,'explicit')",
    );
    let next =
        fixture.execute("INSERT INTO always_default_rejected(label) VALUES ('next') RETURNING id");

    // Assert
    assert!(rejected
        .expect_err("explicit NULL is not DEFAULT")
        .to_string()
        .contains("non-DEFAULT"));
    assert_eq!(
        next.expect("rejection must not allocate defaults").rows(),
        &[vec![Value::Int64(1)]]
    );
    assert_eq!(
        fixture.rows("SELECT count(*) FROM always_default_rejected"),
        vec![vec![Value::Int64(1)]]
    );
}

#[test]
fn should_preserve_excluded_sql_null_given_a_json_default_when_upserting() {
    // Arrange
    let fixture = sql_fixture("upsert-json-null-default", &[
        "CREATE TABLE excluded_json_default (id INT PRIMARY KEY, v JSONB DEFAULT '{\"k\":1}'::jsonb)",
        "INSERT INTO excluded_json_default VALUES (1,'{\"seed\":0}'::jsonb)",
    ]);

    // Act
    let explicit = fixture.execute("INSERT INTO excluded_json_default VALUES (1,NULL) ON CONFLICT(id) DO UPDATE SET v=excluded.v WHERE excluded.v IS NULL RETURNING v IS NULL");
    let defaulted = fixture.execute("INSERT INTO excluded_json_default VALUES (1,DEFAULT) ON CONFLICT(id) DO UPDATE SET v=excluded.v RETURNING v::text");

    // Assert
    assert_eq!(
        explicit
            .expect("EXCLUDED preserves explicit SQL NULL")
            .rows(),
        &[vec![Value::Bool(true)]]
    );
    assert_eq!(
        defaulted.expect("EXCLUDED evaluates DEFAULT").rows(),
        &[vec![Value::String("{\"k\": 1}".into())]]
    );
}

#[test]
fn should_reject_default_marker_spelling_given_ordinary_insert_expressions() {
    // Arrange
    let fixture = sql_fixture(
        "insert-default-marker-boundary",
        &["CREATE TABLE default_marker_boundary (label TEXT DEFAULT 'pending')"],
    );

    // Act
    let marker_call =
        fixture.execute("INSERT INTO default_marker_boundary VALUES (__cassie_column_default())");
    let quoted_column = fixture.execute("INSERT INTO default_marker_boundary VALUES (\"default\")");

    // Assert
    assert!(marker_call.is_err());
    assert!(quoted_column.is_err());
    assert_eq!(
        fixture.rows("SELECT count(*) FROM default_marker_boundary"),
        vec![vec![Value::Int64(0)]]
    );
}

#[test]
fn should_preserve_prepared_defaults_given_deferred_conflict_resolution_at_commit() {
    // Arrange
    let fixture = sql_fixture("insert-default-deferred-intent", &[
        "CREATE TABLE deferred_defaults (id SERIAL PRIMARY KEY, label TEXT, marker SERIAL, v JSONB DEFAULT '{\"k\":1}'::jsonb)",
        "CREATE UNIQUE INDEX deferred_defaults_label ON deferred_defaults(label)",
    ]);
    let first = fixture.cassie.create_session("tester", None);
    let second = fixture.cassie.create_session("tester", None);
    let execute = |session: &cassie::app::CassieSession, sql: &str| {
        fixture.cassie.execute_sql(session, sql, vec![]).expect(sql)
    };
    execute(&first, "BEGIN");
    execute(&second, "BEGIN");
    execute(&first, "INSERT INTO deferred_defaults(label) VALUES ('same') ON CONFLICT(label) DO UPDATE SET marker=excluded.marker,v=excluded.v");

    // Act
    let provisional = execute(&second, "INSERT INTO deferred_defaults VALUES (DEFAULT,'same',DEFAULT,NULL) ON CONFLICT(label) DO UPDATE SET marker=excluded.marker,v=excluded.v RETURNING id,marker");
    execute(&first, "COMMIT");
    execute(&second, "COMMIT");
    let resolved = fixture.rows("SELECT id,marker,v IS NULL FROM deferred_defaults");
    let next =
        fixture.rows("INSERT INTO deferred_defaults(label) VALUES ('next') RETURNING id,marker");

    // Assert
    assert_eq!(
        provisional.rows(),
        &[vec![Value::Int64(2), Value::Int64(2)]]
    );
    assert_eq!(
        resolved,
        vec![vec![Value::Int64(1), Value::Int64(2), Value::Bool(true)]]
    );
    assert_eq!(next, vec![vec![Value::Int64(3), Value::Int64(3)]]);
    assert_eq!(
        fixture.cassie.metrics()["query"]["current_accounted_memory_bytes"],
        0
    );
}

#[test]
fn should_preserve_sql_null_given_tuple_conflict_assignment_when_defaults_are_prepared() {
    // Arrange
    let fixture = sql_fixture("insert-default-tuple-null", &[
        "CREATE TABLE tuple_default_null (id INT PRIMARY KEY, v JSONB DEFAULT '{\"k\":1}'::jsonb)",
        "INSERT INTO tuple_default_null VALUES (1,'{\"seed\":0}'::jsonb)",
    ]);

    // Act
    let updated = fixture.execute("INSERT INTO tuple_default_null VALUES (1,DEFAULT) ON CONFLICT(id) DO UPDATE SET (v)=(SELECT NULL::jsonb) RETURNING v IS NULL");

    // Assert
    assert_eq!(
        updated
            .expect("tuple assignment must preserve SQL NULL")
            .rows(),
        &[vec![Value::Bool(true)]]
    );
    assert_eq!(
        fixture.rows("SELECT v IS NULL FROM tuple_default_null"),
        vec![vec![Value::Bool(true)]]
    );
}
