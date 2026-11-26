#[test]
fn should_report_created_index_definition_in_pg_indexes() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-pg-indexes-created-index",
        &[
            "CREATE SCHEMA issue_817_index_catalog",
            "CREATE TABLE issue_817_index_catalog.items (amount INTEGER)",
            "CREATE INDEX issue_817_catalog_amount_idx \
             ON issue_817_index_catalog.items USING btree (amount)",
        ],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT schemaname, tablename, indexname, indexdef \
             FROM pg_catalog.pg_indexes \
             WHERE schemaname = 'issue_817_index_catalog' \
               AND indexname = 'issue_817_catalog_amount_idx'",
        )
        .expect("query the created index catalog row");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::String("issue_817_index_catalog".to_owned()),
            Value::String("items".to_owned()),
            Value::String("issue_817_catalog_amount_idx".to_owned()),
            Value::String(
                "CREATE INDEX issue_817_catalog_amount_idx ON issue_817_index_catalog.items USING btree (amount)"
                    .to_owned(),
            ),
        ]]
    );
}

fn assert_legacy_index_rows(rows: &[Vec<Value>], expected: [(&str, &str); 3]) {
    assert_eq!(rows.len(), expected.len());
    for (row, (name, key)) in rows.iter().zip(expected) {
        assert_eq!(row[0], Value::String(name.into()));
        assert_eq!(row[1], Value::String(key.into()));
    }
    assert_ne!(rows[0][2], rows[1][2]);
    assert_ne!(rows[1][2], rows[2][2]);
    assert_ne!(rows[0][2], rows[2][2]);
}

fn stored_catalog_oid(cassie: &cassie::Cassie, collection: &str, name: &str) -> Option<u32> {
    cassie
        .midge
        .get_index(collection, name)
        .expect("read stored index")
        .expect("index exists")
        .catalog_oid()
}

fn install_legacy_index_collision(
    cassie: &cassie::Cassie,
    collection: &str,
    registration_order: [&str; 2],
    shared_fallback_oid: u32,
) {
    // Standalone CREATE INDEX stores a scoped index name. Check the candidate
    // from each durable record's actual name, in startup registration order.
    for name in registration_order {
        let mut index = cassie
            .midge
            .get_index(collection, name)
            .expect("read durable index")
            .expect("index");
        assert_eq!(
            cassie_core::catalog::object_identity::stable_index_oid(&format!(
                "{}.{}",
                index.collection, index.name
            )),
            shared_fallback_oid,
        );
        assert!(index.options.remove("__cassie_catalog_oid").is_some());
        cassie
            .midge
            .put_index(&index)
            .expect("store legacy identity");
        cassie.catalog.unregister_index(collection, name);
        cassie.catalog.register_index(index);
    }
}

#[test]
fn should_join_colliding_legacy_index_oids_after_catalog_restart() {
    // Arrange
    let directory = crate::support_temp_dirs::IsolatedDataDir::new("issue-1518-legacy-index-oids");
    let config =
        crate::support_sql::with_test_root_password(cassie::config::CassieRuntimeConfig::default());
    let cassie =
        cassie::Cassie::new_local_with_data_dir_and_config(directory.path(), config.clone())
            .expect("open local Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE orders (col1 INT, col2 INT, col3 INT)",
        "CREATE INDEX idx_legacy_yttnwseryd ON orders (col1)",
        "CREATE INDEX idx_legacy_xsmdnoggin ON orders (col2)",
        "CREATE INDEX idx_allocated_control ON orders (col3)",
    ] {
        cassie
            .execute_sql(&session, sql, vec![])
            .expect("create index fixture");
    }
    let join = "SELECT c.relname, i.indkey, c.oid::text \
                FROM pg_catalog.pg_index i JOIN pg_catalog.pg_class c ON i.indexrelid = c.oid \
                WHERE c.relname IN ('idx_allocated_control', 'idx_legacy_yttnwseryd', 'idx_legacy_xsmdnoggin') \
                ORDER BY c.relname, i.indkey";
    let fresh = cassie
        .execute_sql(&session, join, vec![])
        .expect("read fresh indexes");
    let collection = cassie.midge.canonical_collection_name("orders");
    let survivor = "idx_legacy_yttnwseryd";
    let original_holder = "idx_legacy_xsmdnoggin";
    let shared_fallback_oid = 4_065_978_230;
    install_legacy_index_collision(
        &cassie,
        &collection,
        [original_holder, survivor],
        shared_fallback_oid,
    );

    // Act
    let legacy = cassie
        .execute_sql(&session, join, vec![])
        .expect("read legacy indexes");
    drop(session);
    cassie.shutdown();
    drop(cassie);
    let reopened =
        cassie::Cassie::new_local_with_data_dir_and_config(directory.path(), config.clone())
            .expect("reopen local Cassie");
    reopened.startup().expect("hydrate legacy index identity");
    let session = reopened.create_session("tester", None);
    let hydrated = reopened
        .execute_sql(&session, join, vec![])
        .expect("join reopened indexes");

    // Assert
    let expected = [
        ("idx_allocated_control", "3"),
        (original_holder, "2"),
        (survivor, "1"),
    ];
    for rows in [fresh.rows(), legacy.rows(), hydrated.rows()] {
        assert_legacy_index_rows(rows, expected);
    }
    assert_eq!(fresh.rows()[0], legacy.rows()[0]);
    assert_eq!(legacy.rows(), hydrated.rows());
    let surviving_oid = stored_catalog_oid(&reopened, &collection, survivor)
        .expect("displaced legacy OID was persisted");
    assert_ne!(surviving_oid, shared_fallback_oid);
    assert_eq!(
        hydrated.rows()[2][2],
        Value::String(surviving_oid.to_string())
    );
    assert_eq!(
        stored_catalog_oid(&reopened, &collection, original_holder),
        None
    );
    reopened
        .execute_sql(&session, &format!("DROP INDEX {original_holder}"), vec![])
        .expect("drop the original candidate owner");
    drop(session);
    reopened.shutdown();
    drop(reopened);

    let reopened = cassie::Cassie::new_local_with_data_dir_and_config(directory.path(), config)
        .expect("reopen after candidate owner drop");
    reopened.startup().expect("retain displaced legacy OID");
    let session = reopened.create_session("tester", None);
    let remaining = reopened
        .execute_sql(&session, join, vec![])
        .expect("join surviving indexes");
    assert_eq!(
        remaining.rows(),
        [hydrated.rows()[0].clone(), hydrated.rows()[2].clone()]
    );
    reopened.shutdown();
}

#[test]
fn should_reuse_the_index_oid_when_alter_repeats_a_unique_constraint() {
    // Arrange
    let add = "ALTER TABLE repeat_constraints ADD CONSTRAINT repeated_key UNIQUE (code)";
    let fixture = sql_fixture(
        "issue-1518-repeat-constraint",
        &["CREATE TABLE repeat_constraints (code INTEGER)", add],
    );
    let select = "SELECT oid::text FROM pg_catalog.pg_class WHERE relname = 'repeated_key'";
    let previous = fixture.execute(select).expect("read original index OID");
    let catalog_oid = stored_catalog_oid(&fixture.cassie, "repeat_constraints", "repeated_key");

    // Act
    fixture
        .execute(add)
        .expect("repeat the same accepted ALTER");
    let repeated = fixture.execute(select).expect("read preserved index OID");

    // Assert
    assert_eq!(previous.rows().len(), 1);
    assert!(catalog_oid.is_some());
    assert_eq!(repeated.rows(), previous.rows());
    assert_eq!(
        stored_catalog_oid(&fixture.cassie, "repeat_constraints", "repeated_key"),
        catalog_oid
    );
}

#[test]
fn should_preserve_fresh_constraint_and_cloned_index_oids_after_catalog_restart() {
    // Arrange
    let directory = crate::support_temp_dirs::IsolatedDataDir::new("issue-1518-fresh-index-oids");
    let config =
        crate::support_sql::with_test_root_password(cassie::config::CassieRuntimeConfig::default());
    let cassie =
        cassie::Cassie::new_local_with_data_dir_and_config(directory.path(), config.clone())
            .expect("open local Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE orders (a INT, b INT, c INT, CONSTRAINT idx_probe_mruviborgh UNIQUE (a), CONSTRAINT idx_probe_khsnofgfon UNIQUE (b))",
        "ALTER TABLE orders ADD CONSTRAINT orders_c_unique UNIQUE (c)",
        "CREATE INDEX orders_extra_idx ON orders (a, b)",
        "CREATE TABLE order_copies (LIKE orders INCLUDING INDEXES)",
    ] {
        cassie.execute_sql(&session, sql, vec![]).expect("create fresh index fixture");
    }
    let join = concat!(
        "SELECT t.relname, c.relname, c.oid::text FROM pg_catalog.pg_index i ",
        "JOIN pg_catalog.pg_class c ON c.oid = i.indexrelid ",
        "JOIN pg_catalog.pg_class t ON t.oid = i.indrelid ",
        "WHERE t.relname IN ('orders', 'order_copies') ORDER BY t.relname, c.relname",
    );
    let live = cassie
        .execute_sql(&session, join, vec![])
        .expect("read fresh catalog indexes");
    drop(session);
    cassie.shutdown();
    drop(cassie);

    // Act
    let reopened = cassie::Cassie::new_local_with_data_dir_and_config(directory.path(), config)
        .expect("reopen local Cassie");
    reopened.startup().expect("hydrate fresh index identities");
    let session = reopened.create_session("tester", None);
    let hydrated = reopened
        .execute_sql(&session, join, vec![])
        .expect("read reopened catalog indexes");

    // Assert
    assert_eq!(live.rows().len(), 8);
    let mut oids = std::collections::BTreeSet::new();
    for row in live.rows() {
        let Value::String(oid) = &row[2] else {
            panic!("expected catalog OID text: {row:?}");
        };
        let oid = oid.parse::<u32>().expect("catalog OID");
        assert!(oid < 0x8000_0000, "fresh index uses legacy OID: {row:?}");
        assert!(oids.insert(oid), "multiple fresh indexes share {oid}");
    }
    assert_eq!(live.rows(), hydrated.rows());
    reopened.shutdown();
}

#[test]
fn should_quote_index_definition_identifiers_that_need_quoting() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-pg-indexes-quoted-identifiers",
        &[
            "CREATE SCHEMA \"Issue817MixedCase\"",
            "CREATE TABLE \"Issue817MixedCase\".\"SelectTable\" (\
             \"select\" INTEGER, \"CamelInclude\" INTEGER)",
            "CREATE INDEX \"IndexName\" \
             ON \"Issue817MixedCase\".\"SelectTable\" (\"select\") \
             INCLUDE (\"CamelInclude\")",
        ],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT indexdef FROM pg_catalog.pg_indexes \
             WHERE schemaname = 'Issue817MixedCase' \
               AND tablename = 'SelectTable' \
               AND indexname = 'IndexName'",
        )
        .expect("query the quoted index definition");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::String(
            "CREATE INDEX \"IndexName\" ON \"Issue817MixedCase\".\"SelectTable\" USING btree (\"select\") INCLUDE (\"CamelInclude\")"
                .to_owned(),
        )]]
    );
}

#[test]
fn should_list_builtin_access_methods_in_pg_am() {
    // Arrange
    let fixture = sql_fixture("issue-817-access-method-catalog", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT amname FROM pg_catalog.pg_am \
             WHERE amname IN ('heap', 'btree', 'hash') ORDER BY amname",
        )
        .expect("read PostgreSQL table and index access methods");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![
            vec![Value::String("btree".to_owned())],
            vec![Value::String("hash".to_owned())],
            vec![Value::String("heap".to_owned())],
        ]
    );
}

#[test]
fn should_omit_cassie_internal_index_methods_from_postgresql_catalogs() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-internal-index-catalog-boundary",
        &[
            "CREATE SCHEMA issue_817_internal_index_catalog",
            "CREATE TABLE issue_817_internal_index_catalog.events (\
                 event_at TIMESTAMPTZ NOT NULL, tenant TEXT NOT NULL, payload TEXT)",
            "CREATE INDEX issue_817_events_column_idx \
                 ON issue_817_internal_index_catalog.events USING column (payload) \
                 WITH (segment_size = 1)",
            "CREATE INDEX issue_817_events_timeseries_idx \
                 ON issue_817_internal_index_catalog.events USING time_series (event_at) \
                 WITH (bucket_width = '1 hour', partition_by = tenant)",
            "CREATE INDEX issue_817_events_btree_idx \
                 ON issue_817_internal_index_catalog.events USING btree (tenant)",
        ],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let class_rows = fixture
        .execute(
            "SELECT relname FROM pg_catalog.pg_class \
             WHERE relname IN (\
                 'issue_817_events_column_idx', \
                 'issue_817_events_timeseries_idx', \
                 'issue_817_events_btree_idx') \
             ORDER BY relname",
        )
        .expect("read selected pg_class index rows");
    let access_method_rows = fixture
        .execute(
            "SELECT index_class.relname, index_class.relkind, index_class.relam, \
                    access_method.amname \
             FROM pg_catalog.pg_class AS index_class \
             JOIN pg_catalog.pg_am AS access_method \
               ON access_method.oid = index_class.relam \
             WHERE index_class.relname IN (\
                 'issue_817_events_column_idx', \
                 'issue_817_events_timeseries_idx', \
                 'issue_817_events_btree_idx') \
             ORDER BY index_class.relname",
        )
        .expect("join emitted index relations to genuine access methods");
    let index_rows = fixture
        .execute(
            "SELECT index_class.relname FROM pg_catalog.pg_index index_row \
             JOIN pg_catalog.pg_class index_class ON index_row.indexrelid = index_class.oid \
             JOIN pg_catalog.pg_namespace index_namespace ON index_class.relnamespace = index_namespace.oid \
             WHERE index_class.relname IN (\
                 'issue_817_events_column_idx', \
                 'issue_817_events_timeseries_idx', \
                 'issue_817_events_btree_idx') \
               AND index_namespace.nspname = 'issue_817_internal_index_catalog' \
             ORDER BY index_class.relname",
        )
        .expect("read selected pg_index rows");
    let index_views = fixture
        .execute(
            "SELECT indexname FROM pg_catalog.pg_indexes \
             WHERE schemaname = 'issue_817_internal_index_catalog' \
               AND indexname IN (\
                 'issue_817_events_column_idx', \
                 'issue_817_events_timeseries_idx', \
                 'issue_817_events_btree_idx') \
             ORDER BY indexname",
        )
        .expect("read selected pg_indexes rows");

    // Assert
    let expected = vec![vec![Value::String("issue_817_events_btree_idx".to_owned())]];
    assert_eq!(class_rows.rows(), expected);
    assert_eq!(
        access_method_rows.rows(),
        vec![vec![
            Value::String("issue_817_events_btree_idx".to_owned()),
            Value::String("i".to_owned()),
            Value::Int64(403),
            Value::String("btree".to_owned()),
        ]]
    );
    assert_eq!(index_rows.rows(), expected);
    assert_eq!(index_views.rows(), expected);
}

#[test]
fn should_render_a_created_index_definition() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-index-definition",
        &[
            "CREATE TABLE issue_817_definition_items (amount INTEGER)",
            "CREATE INDEX issue_817_definition_idx \
             ON issue_817_definition_items USING btree (amount)",
        ],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT pg_catalog.pg_get_indexdef(oid) \
             FROM pg_catalog.pg_class \
             WHERE relname = 'issue_817_definition_idx'",
        )
        .expect("render the created index");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::String(
            "CREATE INDEX issue_817_definition_idx ON public.issue_817_definition_items USING btree (amount)"
                .to_owned()
        )]]
    );
}

#[test]
fn should_return_full_definition_given_column_zero_when_calling_pg_get_indexdef() {
    // Arrange
    let fixture = sql_fixture(
        "issue-1486-pg-get-indexdef-full",
        &[
            "CREATE TABLE issue_1486_index_items (amount INTEGER)",
            "CREATE INDEX issue_1486_amount_idx ON issue_1486_index_items (amount)",
        ],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT pg_catalog.pg_get_indexdef(oid, 0, true) \
             FROM pg_catalog.pg_class WHERE relname = 'issue_1486_amount_idx'",
        )
        .expect("render the full index definition");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::String(
            "CREATE INDEX issue_1486_amount_idx ON public.issue_1486_index_items USING btree (amount)"
                .to_owned(),
        )]]
    );
}

#[test]
fn should_return_key_name_given_positive_column_when_calling_pg_get_indexdef() {
    // Arrange
    let fixture = sql_fixture(
        "issue-1486-pg-get-indexdef-key-name",
        &[
            "CREATE TABLE issue_1486_key_items (amount INTEGER)",
            "CREATE INDEX issue_1486_key_idx ON issue_1486_key_items (amount)",
        ],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT pg_get_indexdef(oid, 1, true) \
             FROM pg_catalog.pg_class WHERE relname = 'issue_1486_key_idx'",
        )
        .expect("read the indexed key name");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![Value::String("amount".to_owned())]]
    );
}

#[test]
fn should_return_empty_text_given_column_beyond_index_keys_when_calling_pg_get_indexdef() {
    // Arrange
    let fixture = sql_fixture(
        "issue-1486-pg-get-indexdef-out-of-range",
        &[
            "CREATE TABLE issue_1486_range_items (amount INTEGER)",
            "CREATE INDEX issue_1486_range_idx ON issue_1486_range_items (amount)",
        ],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT pg_get_indexdef(oid, 2, true) \
             FROM pg_catalog.pg_class WHERE relname = 'issue_1486_range_idx'",
        )
        .expect("read a column beyond the index keys");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::String(String::new())]]);
}

#[test]
fn should_reject_four_arguments_when_calling_pg_get_indexdef() {
    // Arrange
    let fixture = sql_fixture("issue-1486-pg-get-indexdef-extra-argument", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let result = fixture.execute("SELECT pg_get_indexdef(1, 0, true, false)");

    // Assert
    assert_eq!(
        result.expect_err("reject an extra argument").sql_state(),
        "42883"
    );
}

#[test]
fn should_reject_non_boolean_pretty_flag_when_calling_pg_get_indexdef() {
    // Arrange
    let fixture = sql_fixture("issue-1486-pg-get-indexdef-wrong-flag", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let result = fixture.execute("SELECT pg_get_indexdef(1, 0, 'true')");

    // Assert
    assert_eq!(
        result.expect_err("reject a text pretty flag").sql_state(),
        "42883"
    );
}

#[test]
fn should_join_supported_catalog_oid_fields_for_psql_describe_queries() {
    // Arrange
    let fixture = sql_fixture(
        "issue-1489-psql-oid-joins",
        &[
            "CREATE SCHEMA issue_1489_first",
            "CREATE SCHEMA issue_1489_second",
            "CREATE TABLE issue_1489_first.items (id INTEGER PRIMARY KEY, amount INTEGER DEFAULT 7)",
            "CREATE TABLE issue_1489_second.items (id INTEGER PRIMARY KEY, amount INTEGER DEFAULT 9)",
            "CREATE INDEX issue_1489_first_amount_idx ON issue_1489_first.items (amount)",
            "CREATE INDEX issue_1489_second_amount_idx ON issue_1489_second.items (amount)",
        ],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT \
                (SELECT count(*) FROM pg_catalog.pg_class c \
                 JOIN pg_catalog.pg_namespace n ON c.relnamespace = n.oid \
                 WHERE c.relname = 'items' AND n.nspname LIKE 'issue_1489_%'), \
                (SELECT count(*) FROM pg_catalog.pg_class c \
                 JOIN pg_catalog.pg_am am ON c.relam = am.oid \
                 WHERE c.relname LIKE 'issue_1489_%_idx' AND am.amname = 'btree'), \
                (SELECT count(*) FROM pg_catalog.pg_index i \
                 JOIN pg_catalog.pg_class index_class ON i.indexrelid = index_class.oid \
                 JOIN pg_catalog.pg_class table_class ON i.indrelid = table_class.oid \
                 WHERE table_class.relname = 'items'), \
                (SELECT count(*) FROM pg_catalog.pg_attrdef d \
                 JOIN pg_catalog.pg_attribute a ON d.adrelid = a.attrelid \
                 WHERE d.adnum = a.attnum), \
                (SELECT count(*) FROM pg_catalog.pg_constraint con \
                 JOIN pg_catalog.pg_index i \
                   ON con.conrelid = i.indrelid AND con.conindid = i.indexrelid \
                 WHERE con.contype = 'p'), \
                (SELECT count(*) FROM pg_catalog.pg_class c \
                 JOIN pg_catalog.pg_class toast ON c.reltoastrelid = toast.oid \
                 WHERE c.relname = 'items'), \
                (SELECT count(*) FROM pg_catalog.pg_publication_rel pr \
                 JOIN pg_catalog.pg_class c ON pr.prrelid = c.oid)",
        )
        .expect("execute supported psql catalog joins");
    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::Int64(2),
            Value::Int64(2),
            Value::Int64(4),
            Value::Int64(2),
            Value::Int64(2),
            Value::Int64(0),
            Value::Int64(0),
        ]]
    );
}

#[test]
fn should_reject_two_arguments_when_calling_pg_get_indexdef() {
    // Arrange
    let fixture = sql_fixture("issue-1486-indexdef-two-arguments", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let result = fixture.execute("SELECT pg_get_indexdef(0, 0)");

    // Assert
    assert_eq!(
        result.expect_err("reject two arguments").sql_state(),
        "42883"
    );
}
