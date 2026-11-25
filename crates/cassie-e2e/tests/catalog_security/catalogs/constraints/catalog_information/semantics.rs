#[test]
fn should_render_supported_constraint_definitions_from_pg_catalog_oids() {
    // Arrange
    let fixture = sql_fixture(
        "issue-1487-constraint-definitions",
        &[
            "CREATE TABLE constraint_parent (id INTEGER PRIMARY KEY)",
            "CREATE TABLE constraint_child (id INTEGER PRIMARY KEY, code TEXT UNIQUE, \
             amount INTEGER CHECK (amount > 0), parent_id INTEGER REFERENCES constraint_parent(id))",
        ],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT contype, pg_get_constraintdef(oid), \
                    pg_catalog.pg_get_constraintdef(oid, true) \
             FROM pg_constraint \
             WHERE conrelid = 'constraint_child'::regclass \
               AND contype IN ('p', 'u', 'c', 'f') \
             ORDER BY contype",
        )
        .expect("render constraint definitions");
    let nulls = fixture
        .execute("SELECT pg_get_constraintdef(NULL), pg_get_constraintdef(999999999)")
        .expect("render null constraint definitions");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![
            vec![
                Value::String("c".to_owned()),
                Value::String("CHECK ((amount > 0))".to_owned()),
                Value::String("CHECK ((amount > 0))".to_owned()),
            ],
            vec![
                Value::String("f".to_owned()),
                Value::String(
                    "FOREIGN KEY (parent_id) REFERENCES constraint_parent(id)".to_owned()
                ),
                Value::String(
                    "FOREIGN KEY (parent_id) REFERENCES constraint_parent(id)".to_owned()
                ),
            ],
            vec![
                Value::String("p".to_owned()),
                Value::String("PRIMARY KEY (id)".to_owned()),
                Value::String("PRIMARY KEY (id)".to_owned()),
            ],
            vec![
                Value::String("u".to_owned()),
                Value::String("UNIQUE (code)".to_owned()),
                Value::String("UNIQUE (code)".to_owned()),
            ],
        ]
    );
    assert_eq!(nulls.rows(), vec![vec![Value::Null, Value::Null]]);
}

#[test]
fn should_report_created_table_constraint_identities() {
    // Arrange
    let fixture = created_object_catalog_fixture("issue-817-created-table-constraints");

    // Act
    let constraints = fixture
        .execute(
            "SELECT con.conname, con.contype::text, con.convalidated \
             FROM pg_catalog.pg_constraint con \
             JOIN pg_catalog.pg_class c ON con.conrelid = c.oid \
             JOIN pg_catalog.pg_namespace n ON c.relnamespace = n.oid \
             WHERE c.relname = 'items' AND n.nspname = 'issue_817_catalog_probe' \
             ORDER BY con.conname",
        )
        .expect("read table constraints");

    // Assert
    assert_eq!(
        constraints.rows(),
        vec![
            vec![
                Value::String("issue_817_amount_positive".to_owned()),
                Value::String("c".to_owned()),
                Value::Bool(true),
            ],
            vec![
                Value::String("items_amount_not_null".to_owned()),
                Value::String("n".to_owned()),
                Value::Bool(true),
            ],
            vec![
                Value::String("items_id_not_null".to_owned()),
                Value::String("n".to_owned()),
                Value::Bool(true),
            ],
            vec![
                Value::String("items_pkey".to_owned()),
                Value::String("p".to_owned()),
                Value::Bool(true),
            ],
        ]
    );
}

#[test]
fn should_emit_typed_zero_when_constraint_has_no_backing_index() {
    // Arrange
    let fixture = sql_fixture(
        "issue-1486-constraint-missing-index-oid",
        &["CREATE TABLE constraint_oid_probe (amount INTEGER CHECK (amount > 0))"],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT conindid, conindid::text FROM pg_constraint \
             WHERE conrelid = 'constraint_oid_probe'::regclass",
        )
        .expect("read absent backing index OID");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::Int64(0), Value::String("0".to_owned())]]
    );
}

#[test]
fn should_discover_the_visible_relation_for_psql_describe() {
    // Arrange
    let fixture = sql_fixture(
        "issue-1487-psql-discovery",
        &[
            "CREATE TABLE probe_docs (id INTEGER PRIMARY KEY)",
            "CREATE TABLE probe_docs_archive (id INTEGER PRIMARY KEY)",
        ],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let discovered = fixture
        .execute(
            "SELECT c.oid, n.nspname, c.relname
             FROM pg_catalog.pg_class c
             LEFT JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
             WHERE c.relname OPERATOR(pg_catalog.~) '^(probe_docs)$' COLLATE pg_catalog.default
               AND pg_catalog.pg_table_is_visible(c.oid)
             ORDER BY 2, 3",
        )
        .expect("discover the relation for psql describe");

    // Assert
    let rows = discovered.rows();
    assert_eq!(rows.len(), 1);
    let relation = &rows[0];
    assert_ne!(relation[0], Value::Null);
    assert_eq!(
        &relation[1..],
        &[
            Value::String("public".to_owned()),
            Value::String("probe_docs".to_owned()),
        ]
    );
}
