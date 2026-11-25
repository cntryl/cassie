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
            "CREATE SCHEMA outside_search_path",
            "CREATE TABLE outside_search_path.probe_docs (id INTEGER PRIMARY KEY)",
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

#[test]
fn should_describe_relation_attributes_and_constraints_for_psql() {
    // Arrange
    let fixture = sql_fixture("psql-describe-catalog", &[
        "CREATE TABLE probe_parent (id INTEGER PRIMARY KEY)",
        "CREATE TABLE probe_docs (id INTEGER PRIMARY KEY, amount INTEGER DEFAULT 7 CHECK (amount > 0), parent_id INTEGER REFERENCES probe_parent(id))",
        "CREATE INDEX probe_docs_amount_idx ON probe_docs (amount)",
    ]);
    fixture.cassie.startup().expect("startup");

    // Act
    let relation = fixture.execute(
        "SELECT relchecks, relhasindex, relhasrules, relhastriggers, relrowsecurity,
                relforcerowsecurity, relispartition, reltablespace, reloftype, relreplident, relpartbound
         FROM pg_catalog.pg_class WHERE oid = 'probe_docs'::regclass"
    ).expect("relation details");
    let attributes = fixture.rows(
        "SELECT a.attname, pg_catalog.format_type(a.atttypid, a.atttypmod),
         (SELECT pg_catalog.pg_get_expr(d.adbin, d.adrelid, true) FROM pg_catalog.pg_attrdef d
          WHERE d.adrelid = a.attrelid AND d.adnum = a.attnum AND a.atthasdef),
         a.attnotnull,
         (SELECT c.collname FROM pg_catalog.pg_collation c, pg_catalog.pg_type t
          WHERE c.oid = a.attcollation AND t.oid = a.atttypid AND a.attcollation <> t.typcollation)
         FROM pg_catalog.pg_attribute a WHERE a.attrelid = 'probe_docs'::regclass AND a.attnum > 0 AND NOT a.attisdropped
         ORDER BY a.attnum"
    );
    let index_rows = fixture.rows(
        "SELECT c.relname, i.indisprimary, i.indisclustered, i.indisreplident, c.reltablespace
         FROM pg_catalog.pg_index i JOIN pg_catalog.pg_class c ON i.indexrelid = c.oid
         WHERE i.indrelid = 'probe_docs'::regclass ORDER BY i.indisprimary DESC, c.relname",
    );
    let keys = fixture.rows(
        "SELECT contype, conparentid, conperiod FROM pg_catalog.pg_constraint
         WHERE conrelid = 'probe_docs'::regclass AND contype IN ('c','f','p') ORDER BY contype",
    );

    // Assert
    assert_eq!(
        relation.rows(),
        vec![vec![
            Value::Int64(1),
            Value::Bool(true),
            Value::Bool(false),
            Value::Bool(false),
            Value::Bool(false),
            Value::Bool(false),
            Value::Bool(false),
            Value::Int64(0),
            Value::Int64(0),
            Value::String("d".into()),
            Value::Null,
        ]]
    );
    assert_eq!(relation.columns()[0].type_oid, 21);
    assert_eq!(relation.columns()[7].type_oid, 26);
    assert_eq!(relation.columns()[8].type_oid, 26);
    assert_eq!(
        attributes,
        vec![
            vec![
                Value::String("id".into()),
                Value::String("integer".into()),
                Value::Null,
                Value::Bool(true),
                Value::Null
            ],
            vec![
                Value::String("amount".into()),
                Value::String("integer".into()),
                Value::String("7".into()),
                Value::Bool(false),
                Value::Null
            ],
            vec![
                Value::String("parent_id".into()),
                Value::String("integer".into()),
                Value::Null,
                Value::Bool(false),
                Value::Null
            ],
        ]
    );
    assert_eq!(
        index_rows,
        ["probe_docs_pkey", "probe_docs_amount_idx"].map(|name| vec![
            Value::String(name.into()),
            Value::Bool(name.ends_with("_pkey")),
            Value::Bool(false),
            Value::Bool(false),
            Value::Int64(0)
        ])
    );
    assert_eq!(
        keys,
        ["c", "f", "p"].map(|kind| vec![
            Value::String(kind.into()),
            Value::Int64(0),
            Value::Bool(false)
        ])
    );
}

#[test]
fn should_return_no_partition_ancestors_for_ordinary_tables_in_psql_foreign_key_lookups() {
    // Arrange
    let fixture = sql_fixture(
        "psql-foreign-key-ancestors",
        &[
            "CREATE TABLE ordinary_parent (id INT PRIMARY KEY)",
            "CREATE TABLE ordinary_child (parent_id INT CONSTRAINT ordinary_child_parent_id_fkey REFERENCES ordinary_parent(id))",
        ],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let ancestors = fixture
        .execute("SELECT pg_catalog.pg_partition_ancestors('ordinary_parent')")
        .unwrap();
    let source = fixture.rows("SELECT relid, depth FROM pg_catalog.pg_partition_ancestors('ordinary_parent') WITH ORDINALITY AS a(relid, depth)");
    let referencing = fixture.rows("SELECT conname FROM pg_catalog.pg_constraint WHERE confrelid IN (
        SELECT pg_catalog.pg_partition_ancestors('ordinary_parent') UNION ALL VALUES ('ordinary_parent'::regclass)
    ) AND contype = 'f' AND conparentid = 0");
    let missing_name =
        fixture.execute("SELECT pg_catalog.pg_partition_ancestors('no_such_parent')");

    // Assert
    assert!(ancestors.rows().is_empty());
    assert_eq!(ancestors.columns()[0].type_oid, 2205);
    assert!(source.is_empty());
    assert_eq!(
        referencing,
        vec![vec![Value::String("ordinary_child_parent_id_fkey".into())]]
    );
    assert!(missing_name.is_err());
}

#[test]
fn should_bind_partition_ancestor_arguments_using_the_regclass_input_signature() {
    // Arrange
    let fixture = sql_fixture(
        "ancestor-input-types",
        &["CREATE TABLE ancestor_inputs (id INTEGER)"],
    );
    let accepted = [
        "'ancestor_inputs'::text",
        "'ancestor_inputs'::varchar(32)",
        "'ancestor_inputs'::regclass",
        "1::smallint",
        "1::integer",
        "1::bigint",
        "1::oid",
        "NULL",
    ];
    let rejected = [
        "1::regtype",
        "1::regnamespace",
        "true",
        "1::numeric",
        "'ancestor_inputs'::name",
        "'ancestor_inputs'::char(32)",
    ];

    // Act
    for argument in accepted {
        let sql =
            format!("SELECT relid FROM pg_catalog.pg_partition_ancestors({argument}) AS a(relid)");
        let result = fixture
            .execute(&sql)
            .unwrap_or_else(|err| panic!("{sql}: {err}"));

        // Assert
        assert_eq!(result.columns()[0].type_oid, 2205);
        assert!(result.rows().is_empty(), "{sql}");
    }
    for argument in rejected {
        let sql = format!("SELECT pg_catalog.pg_partition_ancestors({argument})");
        let error = fixture
            .execute(&sql)
            .expect_err("incompatible regclass argument");

        // Assert
        assert_eq!(error.descriptor().sql_state, "42883", "{sql}: {error}");
    }
}

#[test]
fn should_list_primary_and_unconstrained_indexes_with_psql_mixed_joins() {
    // Arrange
    let fixture = sql_fixture("psql-index-list-with-left-join", &[
        "CREATE TABLE probe_parent (id INTEGER PRIMARY KEY)",
        "CREATE TABLE probe_docs (id INTEGER PRIMARY KEY, amount INTEGER DEFAULT 7 CHECK (amount > 0), parent_id INTEGER REFERENCES probe_parent(id))",
        "CREATE INDEX probe_docs_amount_idx ON probe_docs (amount)",
        "CREATE TABLE probe_docs_archive (id INTEGER PRIMARY KEY)",
        "CREATE SCHEMA outside_search_path",
        "CREATE TABLE outside_search_path.probe_docs (id INTEGER PRIMARY KEY)",
    ]);
    fixture.cassie.startup().expect("startup");

    // Act
    let rows = fixture.rows(
        "SELECT c2.relname, i.indisprimary, pg_catalog.pg_get_indexdef(i.indexrelid, 0, true),
                pg_catalog.pg_get_constraintdef(con.oid, true), contype, conperiod
         FROM pg_catalog.pg_class c, pg_catalog.pg_class c2, pg_catalog.pg_index i
         LEFT JOIN pg_catalog.pg_constraint con ON
           (conrelid = i.indrelid AND conindid = i.indexrelid AND contype IN ('p','u','x'))
         WHERE c.oid = 'probe_docs'::regclass AND c.oid = i.indrelid AND i.indexrelid = c2.oid
         ORDER BY i.indisprimary DESC, c2.relname",
    );

    // Assert
    assert_eq!(
        rows,
        vec![
            vec![
                Value::String("probe_docs_pkey".into()),
                Value::Bool(true),
                Value::String(
                    "CREATE UNIQUE INDEX probe_docs_pkey ON public.probe_docs USING btree (id)"
                        .into()
                ),
                Value::String("PRIMARY KEY (id)".into()),
                Value::String("p".into()),
                Value::Bool(false)
            ],
            vec![
                Value::String("probe_docs_amount_idx".into()),
                Value::Bool(false),
                Value::String(
                    "CREATE INDEX probe_docs_amount_idx ON public.probe_docs USING btree (amount)"
                        .into()
                ),
                Value::Null,
                Value::Null,
                Value::Null
            ],
        ]
    );
}

#[test]
fn should_bind_an_unknown_array_literal_in_a_projected_policy_comparison() {
    // Arrange
    let fixture = sql_fixture("psql-policy-comparison", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let empty = fixture.rows(
        "SELECT CASE WHEN polroles = '{0}' THEN NULL ELSE polname END FROM pg_catalog.pg_policy",
    );
    let values = fixture.rows(
        "SELECT CASE WHEN roles = '{0}' THEN 1 ELSE 2 END,
        '{0}' = roles FROM (VALUES ('{0}'::oid[]), ('{11}'::oid[])) v(roles) ORDER BY 1",
    );
    let typed_text = fixture.execute("SELECT '{0}'::text = '{0}'::oid[]");

    // Assert
    assert!(empty.is_empty());
    assert_eq!(
        values,
        vec![
            vec![Value::Int64(1), Value::Bool(true)],
            vec![Value::Int64(2), Value::Bool(false)]
        ]
    );
    assert!(typed_text.is_err());
}
