use cassie::app::Cassie;
use cassie::types::Value;
use std::path::PathBuf;
use uuid::Uuid;

fn with_fallback() {
    if std::env::var("CASSIE_EMBEDDINGS_PROVIDER").is_err() {
        std::env::set_var("CASSIE_EMBEDDINGS_PROVIDER", "fallback");
    }
}

fn data_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("cassie-migration-ddl-{name}-{}", Uuid::new_v4()))
}

#[test]
fn should_apply_sequence_defaults_metadata_through_restart() {
    // Arrange
    with_fallback();
    let path = data_dir("sequence-defaults");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);

        cassie
            .execute_sql(&session, "CREATE SEQUENCE order_ids", vec![])
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE migration_orders (
                    seq_id INT DEFAULT nextval('order_ids'::regclass),
                    label TEXT
                )",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE migration_source (label TEXT)",
                vec![],
            )
            .unwrap();

        // Act
        cassie
            .execute_sql(
                &session,
                "INSERT INTO migration_orders (label) VALUES ('alpha')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO migration_source (label) VALUES ('beta')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO migration_orders (label) SELECT label FROM migration_source",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "ALTER TABLE migration_orders ALTER COLUMN label SET DEFAULT 'pending'",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO migration_orders (seq_id) VALUES (10)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "ALTER TABLE migration_orders ALTER COLUMN label SET NOT NULL",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "ALTER TABLE migration_orders ALTER COLUMN label DROP NOT NULL",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "ALTER TABLE migration_orders ALTER COLUMN label DROP DEFAULT",
                vec![],
            )
            .unwrap();

        let before_restart = cassie
            .execute_sql(
                &session,
                "SELECT seq_id, label FROM migration_orders ORDER BY seq_id",
                vec![],
            )
            .unwrap();
        drop(cassie);

        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let restarted_session = restarted.create_session("tester", None);
        restarted
            .execute_sql(
                &restarted_session,
                "INSERT INTO migration_orders (label) VALUES ('gamma')",
                vec![],
            )
            .unwrap();
        restarted
            .execute_sql(&restarted_session, "CREATE SEQUENCE temp_ids", vec![])
            .unwrap();
        restarted
            .execute_sql(&restarted_session, "DROP SEQUENCE temp_ids", vec![])
            .unwrap();

        let after_restart = restarted
            .execute_sql(
                &restarted_session,
                "SELECT seq_id, label FROM migration_orders ORDER BY seq_id",
                vec![],
            )
            .unwrap();
        let columns = restarted
            .execute_sql(
                &restarted_session,
                "SELECT column_name, column_default, is_nullable FROM information_schema.columns WHERE table_name = 'migration_orders' ORDER BY ordinal_position",
                vec![],
            )
            .unwrap();
        let attrdefs = restarted
            .execute_sql(
                &restarted_session,
                "SELECT adrelid, adnum, adsrc FROM pg_catalog.pg_attrdef WHERE adrelid = 'migration_orders' ORDER BY adnum",
                vec![],
            )
            .unwrap();
        let sequences = restarted
            .execute_sql(
                &restarted_session,
                "SELECT sequence_name, data_type, start_value, increment FROM information_schema.sequences WHERE sequence_name = 'order_ids'",
                vec![],
            )
            .unwrap();
        let sequence_class = restarted
            .execute_sql(
                &restarted_session,
                "SELECT relname, relkind FROM pg_catalog.pg_class WHERE relname = 'order_ids'",
                vec![],
            )
            .unwrap();
        let dropped_sequence = restarted
            .execute_sql(
                &restarted_session,
                "SELECT sequence_name FROM information_schema.sequences WHERE sequence_name = 'temp_ids'",
                vec![],
            )
            .unwrap();
        let unsupported = restarted.execute_sql(
            &restarted_session,
            "CREATE SEQUENCE unsupported_ids START WITH 5",
            vec![],
        );

        // Assert
        assert_eq!(
            before_restart.rows,
            vec![
                vec![
                    Value::Int64(1),
                    Value::String("alpha".to_string())
                ],
                vec![Value::Int64(2), Value::String("beta".to_string())],
                vec![Value::Int64(10), Value::String("pending".to_string())],
            ]
        );
        assert_eq!(
            after_restart.rows,
            vec![
                vec![
                    Value::Int64(1),
                    Value::String("alpha".to_string())
                ],
                vec![Value::Int64(2), Value::String("beta".to_string())],
                vec![Value::Int64(3), Value::String("gamma".to_string())],
                vec![Value::Int64(10), Value::String("pending".to_string())],
            ]
        );
        assert_eq!(
            columns.rows,
            vec![
                vec![
                    Value::String("seq_id".to_string()),
                    Value::String("nextval('order_ids'::regclass)".to_string()),
                    Value::String("YES".to_string()),
                ],
                vec![
                    Value::String("label".to_string()),
                    Value::Null,
                    Value::String("YES".to_string()),
                ],
            ]
        );
        assert_eq!(
            attrdefs.rows,
            vec![vec![
                Value::String("migration_orders".to_string()),
                Value::Int64(1),
                Value::String("nextval('order_ids'::regclass)".to_string()),
            ]]
        );
        assert_eq!(
            sequences.rows,
            vec![vec![
                Value::String("order_ids".to_string()),
                Value::String("integer".to_string()),
                Value::String("1".to_string()),
                Value::String("1".to_string()),
            ]]
        );
        assert_eq!(
            sequence_class.rows,
            vec![vec![
                Value::String("order_ids".to_string()),
                Value::String("S".to_string()),
            ]]
        );
        assert!(dropped_sequence.rows.is_empty());
        assert!(matches!(
            unsupported,
            Err(error) if error.to_string().contains("unsupported CREATE SEQUENCE option")
        ));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_desugar_serial_columns_to_sequence_backed_integer_defaults() {
    // Arrange
    with_fallback();
    let path = data_dir("serial");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
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
            rows.rows,
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
            columns.rows,
            vec![
                vec![
                    Value::String("serial_id".to_string()),
                    Value::String("int".to_string()),
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
