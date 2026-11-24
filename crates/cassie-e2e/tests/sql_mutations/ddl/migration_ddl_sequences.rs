use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;
use std::path::Path;

use super::support_data_dir as data_dir;
use super::support_local_storage as local_storage;
use data_dir::data_dir;
use local_storage::use_local_storage;

struct SequenceRestartState {
    after_restart: Vec<Vec<Value>>,
    columns: Vec<Vec<Value>>,
    attrdefs: Vec<Vec<Value>>,
    sequences: Vec<Vec<Value>>,
    sequence_class: Vec<Vec<Value>>,
    dropped_sequence: Vec<Vec<Value>>,
    invalid_option_error: String,
}

fn execute_statement(cassie: &Cassie, session: &CassieSession, sql: &str) {
    cassie
        .execute_sql(session, sql, vec![])
        .unwrap_or_else(|error| panic!("execute {sql}: {error}"));
}

fn query_rows(cassie: &Cassie, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(session, sql, vec![])
        .unwrap()
        .rows()
        .to_vec()
}

fn create_sequence_default_schema(cassie: &Cassie, session: &CassieSession) {
    execute_statement(cassie, session, "CREATE SEQUENCE order_ids");
    execute_statement(
        cassie,
        session,
        "CREATE TABLE migration_orders (
            seq_id INT DEFAULT nextval('order_ids'::regclass),
            label TEXT
        )",
    );
    execute_statement(
        cassie,
        session,
        "CREATE TABLE migration_source (label TEXT)",
    );
}

fn apply_sequence_default_mutations(cassie: &Cassie, session: &CassieSession) {
    for sql in [
        "INSERT INTO migration_orders (label) VALUES ('alpha')",
        "INSERT INTO migration_source (label) VALUES ('beta')",
        "INSERT INTO migration_orders (label) SELECT label FROM migration_source",
        "ALTER TABLE migration_orders ALTER COLUMN label SET DEFAULT 'pending'",
        "INSERT INTO migration_orders (seq_id) VALUES (10)",
        "ALTER TABLE migration_orders ALTER COLUMN label SET NOT NULL",
        "ALTER TABLE migration_orders ALTER COLUMN label DROP NOT NULL",
        "ALTER TABLE migration_orders ALTER COLUMN label DROP DEFAULT",
    ] {
        execute_statement(cassie, session, sql);
    }
}

fn migration_orders_rows(cassie: &Cassie, session: &CassieSession) -> Vec<Vec<Value>> {
    query_rows(
        cassie,
        session,
        "SELECT seq_id, label FROM migration_orders ORDER BY seq_id",
    )
}

fn restart_and_collect_sequence_state(path: &Path) -> SequenceRestartState {
    let restarted = Cassie::new_with_data_dir(path).unwrap();
    restarted.startup().unwrap();
    let session = restarted.create_session("tester", None);
    execute_statement(
        &restarted,
        &session,
        "INSERT INTO migration_orders (label) VALUES ('gamma')",
    );
    execute_statement(&restarted, &session, "CREATE SEQUENCE temp_ids");
    execute_statement(&restarted, &session, "DROP SEQUENCE temp_ids");

    let invalid_option = restarted
        .execute_sql(
            &session,
            "CREATE SEQUENCE invalid_ids START WITH 5 MINVALUE 10",
            vec![],
        )
        .expect_err("a start below the minimum should fail")
        .to_string();

    SequenceRestartState {
        after_restart: migration_orders_rows(&restarted, &session),
        columns: query_rows(
            &restarted,
            &session,
            "SELECT column_name, column_default, is_nullable FROM information_schema.columns WHERE table_name = 'migration_orders' ORDER BY ordinal_position",
        ),
        attrdefs: query_rows(
            &restarted,
            &session,
            "SELECT adrelid, adnum, adsrc FROM pg_catalog.pg_attrdef WHERE adrelid = 'migration_orders' ORDER BY adnum",
        ),
        sequences: query_rows(
            &restarted,
            &session,
            "SELECT sequence_name, data_type, start_value, increment FROM information_schema.sequences WHERE sequence_name = 'order_ids'",
        ),
        sequence_class: query_rows(
            &restarted,
            &session,
            "SELECT relname, relkind FROM pg_catalog.pg_class WHERE relname = 'order_ids'",
        ),
        dropped_sequence: query_rows(
            &restarted,
            &session,
            "SELECT sequence_name FROM information_schema.sequences WHERE sequence_name = 'temp_ids'",
        ),
        invalid_option_error: invalid_option,
    }
}

fn assert_sequence_default_state(before_restart: &[Vec<Value>], state: &SequenceRestartState) {
    assert_eq!(before_restart, expected_orders_before_restart().as_slice());
    assert_eq!(state.after_restart, expected_orders_after_restart());
    assert_eq!(state.columns, expected_column_rows());
    assert_eq!(state.attrdefs, expected_attrdef_rows());
    assert_eq!(state.sequences, expected_sequence_rows());
    assert_eq!(state.sequence_class, expected_sequence_class_rows());
    assert_eq!(
        state.dropped_sequence,
        [] as [std::vec::Vec<cassie::types::Value>; 0]
    );
    assert!(state
        .invalid_option_error
        .contains("START value (5) cannot be less than MINVALUE (10)"));
}

fn expected_orders_before_restart() -> Vec<Vec<Value>> {
    vec![
        vec![Value::Int64(1), Value::String("alpha".to_string())],
        vec![Value::Int64(2), Value::String("beta".to_string())],
        vec![Value::Int64(10), Value::String("pending".to_string())],
    ]
}

fn expected_orders_after_restart() -> Vec<Vec<Value>> {
    vec![
        vec![Value::Int64(1), Value::String("alpha".to_string())],
        vec![Value::Int64(2), Value::String("beta".to_string())],
        vec![Value::Int64(3), Value::String("gamma".to_string())],
        vec![Value::Int64(10), Value::String("pending".to_string())],
    ]
}

fn expected_column_rows() -> Vec<Vec<Value>> {
    vec![
        vec![
            Value::String("seq_id".to_string()),
            Value::String("nextval('postgres.public.order_ids'::regclass)".to_string()),
            Value::String("YES".to_string()),
        ],
        vec![
            Value::String("label".to_string()),
            Value::Null,
            Value::String("YES".to_string()),
        ],
    ]
}

fn expected_attrdef_rows() -> Vec<Vec<Value>> {
    vec![vec![
        Value::String("migration_orders".to_string()),
        Value::Int64(1),
        Value::String("nextval('postgres.public.order_ids'::regclass)".to_string()),
    ]]
}

fn expected_sequence_rows() -> Vec<Vec<Value>> {
    vec![vec![
        Value::String("order_ids".to_string()),
        Value::String("bigint".to_string()),
        Value::String("1".to_string()),
        Value::String("1".to_string()),
    ]]
}

fn expected_sequence_class_rows() -> Vec<Vec<Value>> {
    vec![vec![
        Value::String("order_ids".to_string()),
        Value::String("S".to_string()),
    ]]
}

#[test]
fn should_apply_sequence_defaults_metadata_through_restart() {
    // Arrange
    use_local_storage();
    let path = data_dir("sequence-defaults");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        create_sequence_default_schema(&cassie, &session);

        // Act
        apply_sequence_default_mutations(&cassie, &session);
        let before_restart = migration_orders_rows(&cassie, &session);
        drop(cassie);
        let state = restart_and_collect_sequence_state(&path);

        // Assert
        assert_sequence_default_state(&before_restart, &state);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_register_serial_sequence_in_schema_of_qualified_table() {
    // Arrange
    let path = data_dir("serial_schema_qualified_table");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", Some("postgres".to_string()));
        execute_statement(&cassie, &session, "CREATE SCHEMA ledger");

        // Act
        execute_statement(
            &cassie,
            &session,
            "CREATE TABLE ledger.entries (entry_id SERIAL PRIMARY KEY, label TEXT)",
        );
        execute_statement(
            &cassie,
            &session,
            "INSERT INTO ledger.entries (label) VALUES ('one')",
        );
        let sequence_names = cassie
            .catalog
            .list_sequences()
            .into_iter()
            .map(|sequence| sequence.name)
            .collect::<Vec<_>>();
        let rows = query_rows(
            &cassie,
            &session,
            "SELECT entry_id, label FROM ledger.entries",
        );
        execute_statement(&cassie, &session, "DROP TABLE ledger.entries");
        let remaining_sequences = cassie.catalog.list_sequences();

        // Assert
        assert_eq!(
            sequence_names,
            vec!["postgres.ledger.entries_entry_id_seq".to_string()]
        );
        assert_eq!(
            rows,
            vec![vec![Value::Int64(1), Value::String("one".to_string())]]
        );
        assert_eq!(
            remaining_sequences,
            [] as [cassie::catalog::SequenceMeta; 0]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_qualify_serial_sequence_for_unqualified_table_in_search_path_schema() {
    // Arrange
    let path = data_dir("serial_search_path_schema");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let public_session = cassie.create_session("tester", Some("postgres".to_string()));
        execute_statement(&cassie, &public_session, "CREATE SCHEMA reporting");
        execute_statement(
            &cassie,
            &public_session,
            "CREATE TABLE metrics (metric_id SERIAL, label TEXT)",
        );
        let reporting_session = cassie.create_session("tester", Some("postgres".to_string()));
        execute_statement(&cassie, &reporting_session, "SET search_path = reporting");

        // Act
        execute_statement(
            &cassie,
            &reporting_session,
            "CREATE TABLE metrics (metric_id SERIAL, label TEXT)",
        );
        execute_statement(
            &cassie,
            &public_session,
            "INSERT INTO metrics (label) VALUES ('public-one')",
        );
        execute_statement(
            &cassie,
            &reporting_session,
            "INSERT INTO metrics (label) VALUES ('reporting-one')",
        );
        execute_statement(
            &cassie,
            &public_session,
            "ALTER SCHEMA reporting RENAME TO analytics",
        );
        let renamed_insert = cassie.execute_sql(
            &public_session,
            "INSERT INTO analytics.metrics (label) VALUES ('analytics-two')",
            vec![],
        );
        let sequence_names = cassie
            .catalog
            .list_sequences()
            .into_iter()
            .map(|sequence| sequence.name)
            .collect::<Vec<_>>();
        let analytics_rows = query_rows(
            &cassie,
            &public_session,
            "SELECT metric_id, label FROM analytics.metrics ORDER BY metric_id",
        );
        let public_rows = query_rows(
            &cassie,
            &public_session,
            "SELECT metric_id, label FROM public.metrics ORDER BY metric_id",
        );

        // Assert
        assert!(
            sequence_names.contains(&"postgres.analytics.metrics_metric_id_seq".to_string()),
            "sequence was not qualified with the resolved schema: {sequence_names:?}"
        );
        assert!(
            renamed_insert.is_ok(),
            "insert after schema rename failed: {renamed_insert:?}"
        );
        assert_eq!(
            analytics_rows,
            vec![
                vec![Value::Int64(1), Value::String("reporting-one".to_string())],
                vec![Value::Int64(2), Value::String("analytics-two".to_string())],
            ]
        );
        assert_eq!(
            public_rows,
            vec![vec![
                Value::Int64(1),
                Value::String("public-one".to_string())
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_keep_serial_default_working_after_renaming_its_schema() {
    // Arrange
    let path = data_dir("serial_schema_rename");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", Some("postgres".to_string()));
        execute_statement(&cassie, &session, "CREATE SCHEMA billing");
        execute_statement(
            &cassie,
            &session,
            "CREATE TABLE billing.invoices (invoice_id SERIAL PRIMARY KEY, label TEXT)",
        );
        execute_statement(
            &cassie,
            &session,
            "INSERT INTO billing.invoices (label) VALUES ('one')",
        );
        execute_statement(
            &cassie,
            &session,
            "ALTER SCHEMA billing RENAME TO invoicing",
        );

        // Act
        let inserted = cassie.execute_sql(
            &session,
            "INSERT INTO invoicing.invoices (label) VALUES ('two')",
            vec![],
        );
        let rows = query_rows(
            &cassie,
            &session,
            "SELECT invoice_id, label FROM invoicing.invoices ORDER BY invoice_id",
        );

        // Assert
        assert!(
            inserted.is_ok(),
            "insert after schema rename failed: {inserted:?}"
        );
        assert_eq!(
            rows,
            vec![
                vec![Value::Int64(1), Value::String("one".to_string())],
                vec![Value::Int64(2), Value::String("two".to_string())],
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_release_serial_sequence_when_dropping_its_table() {
    // Arrange
    let path = data_dir("serial_sequence_table_drop");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE SCHEMA sx",
            "CREATE TABLE sx.t (k SERIAL, v INT)",
            "INSERT INTO sx.t (v) VALUES (1)",
            "DROP TABLE sx.t",
            "DROP SCHEMA sx",
            "CREATE SCHEMA sx",
            "CREATE TABLE sx.t (k SERIAL, v INT)",
            "INSERT INTO sx.t (v) VALUES (2)",
        ] {
            execute_statement(&cassie, &session, sql);
        }

        // Act
        let rows = query_rows(&cassie, &session, "SELECT k FROM sx.t");

        // Assert
        assert_eq!(rows, vec![vec![Value::Int64(1)]]);
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_rename_serial_sequence_when_renaming_its_table() {
    // Arrange
    use_local_storage();
    let path = data_dir("serial_sequence_table_rename");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE t (k SERIAL, v INT)",
            "INSERT INTO t (v) VALUES (1)",
            "ALTER TABLE t RENAME TO t2",
        ] {
            execute_statement(&cassie, &session, sql);
        }
        assert!(cassie.catalog.sequence_exists("t2_k_seq"));
        assert!(
            !cassie.catalog.sequence_exists("t_k_seq"),
            "old sequence survived table rename: sequences={:?}, constraints={:?}",
            cassie.catalog.list_sequences(),
            cassie.catalog.get_constraints("t2")
        );
        assert_eq!(cassie.catalog.list_sequences().len(), 1);
        let renamed_sequence = cassie
            .catalog
            .get_sequence("t2_k_seq")
            .expect("renamed serial sequence");
        assert_eq!(
            renamed_sequence
                .owned_by
                .as_ref()
                .map(|owner| owner.table.as_str()),
            Some("postgres.public.t2")
        );

        // Act
        drop(session);
        drop(cassie);
        let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
        restarted.startup().expect("restart");
        let session = restarted.create_session("tester", None);
        assert!(restarted.catalog.sequence_exists("t2_k_seq"));
        assert!(!restarted.catalog.sequence_exists("t_k_seq"));

        for sql in [
            "INSERT INTO t2 (v) VALUES (2)",
            "CREATE TABLE t (k SERIAL, v INT)",
            "INSERT INTO t (v) VALUES (3)",
        ] {
            execute_statement(&restarted, &session, sql);
        }

        let renamed_rows = query_rows(&restarted, &session, "SELECT k FROM t2 ORDER BY k");
        let recreated_rows = query_rows(&restarted, &session, "SELECT k FROM t");

        // Assert
        assert_eq!(
            renamed_rows,
            vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]
        );
        assert_eq!(recreated_rows, vec![vec![Value::Int64(1)]]);
        drop(session);
        drop(restarted);
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_desugar_serial_columns_to_sequence_backed_integer_defaults() {
    // Arrange
    let path = data_dir("serial");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE serial_orders (
                    serial_id SERIAL,
                    ledger_id BIGSERIAL,
                    label TEXT
                )",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO serial_orders (label) VALUES ('one')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO serial_orders (label) VALUES ('two')",
                vec![],
            )
            .unwrap();

        let rows = cassie
            .execute_sql(
                &session,
                "SELECT serial_id, ledger_id, label FROM serial_orders ORDER BY serial_id",
                vec![],
            )
            .unwrap();
        let columns = cassie
            .execute_sql(
                &session,
                "SELECT column_name, data_type, column_default, is_nullable FROM information_schema.columns WHERE table_name = 'serial_orders' ORDER BY ordinal_position",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            rows.rows(),
            vec![
                vec![
                    Value::Int64(1),
                    Value::Int64(1),
                    Value::String("one".to_string()),
                ],
                vec![
                    Value::Int64(2),
                    Value::Int64(2),
                    Value::String("two".to_string()),
                ],
            ]
        );
        assert_eq!(
            columns.rows(),
            vec![
                vec![
                    Value::String("serial_id".to_string()),
                    Value::String("integer".to_string()),
                    Value::String("nextval('serial_orders_serial_id_seq'::regclass)".to_string()),
                    Value::String("NO".to_string()),
                ],
                vec![
                    Value::String("ledger_id".to_string()),
                    Value::String("bigint".to_string()),
                    Value::String("nextval('serial_orders_ledger_id_seq'::regclass)".to_string()),
                    Value::String("NO".to_string()),
                ],
                vec![
                    Value::String("label".to_string()),
                    Value::String("text".to_string()),
                    Value::Null,
                    Value::String("YES".to_string()),
                ],
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
#[test]
fn should_bind_bare_explicit_sequence_defaults_to_table_schema() {
    // Arrange
    let path = data_dir("explicit-sequence-default-schema");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE SCHEMA aa",
            "CREATE SCHEMA bb",
            "CREATE SEQUENCE aa.s",
            "CREATE SEQUENCE bb.s",
            "CREATE SEQUENCE aa.altered",
            "CREATE SEQUENCE bb.altered",
            "CREATE TABLE bb.t (v INT, n INT DEFAULT nextval('s'))",
            "CREATE TABLE bb.altered_t (v INT, n INT)",
            "ALTER TABLE bb.altered_t ALTER COLUMN n SET DEFAULT nextval('altered')",
            "INSERT INTO bb.t (v) VALUES (1), (2)",
            "INSERT INTO bb.altered_t (v) VALUES (1), (2)",
            "DROP SEQUENCE aa.s",
            "DROP SEQUENCE aa.altered",
            "INSERT INTO bb.t (v) VALUES (3), (4)",
            "INSERT INTO bb.altered_t (v) VALUES (3), (4)",
        ] {
            execute_statement(&cassie, &session, sql);
        }

        // Act
        let rows = query_rows(&cassie, &session, "SELECT v, n FROM bb.t ORDER BY v");
        let altered_rows = query_rows(
            &cassie,
            &session,
            "SELECT v, n FROM bb.altered_t ORDER BY v",
        );

        // Assert
        let expected_rows = vec![
            vec![Value::Int64(1), Value::Int64(1)],
            vec![Value::Int64(2), Value::Int64(2)],
            vec![Value::Int64(3), Value::Int64(3)],
            vec![Value::Int64(4), Value::Int64(4)],
        ];
        assert_eq!(altered_rows, expected_rows);
        assert_eq!(rows, expected_rows);

        let _ = std::fs::remove_dir_all(path);
    });
}
