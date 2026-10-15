use cassie::app::Cassie;
use cassie::types::Value;

use super::support_sql as support;
use support::{data_dir, use_local_storage};

#[test]
fn should_create_json_index_without_blocking_object_writes() {
    // Arrange
    use_local_storage();
    let path = data_dir("json_index_object_writes");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE json_index_object_writes (id TEXT, doc JSON)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO json_index_object_writes (id, doc) VALUES ('before', $1)",
                vec![Value::Json(serde_json::json!({"k": 1}))],
            )
            .expect("insert before index");

        // Act
        let index = cassie.execute_sql(
            &session,
            "CREATE INDEX json_index_object_writes_doc_idx ON json_index_object_writes (doc)",
            vec![],
        );
        let insert = cassie.execute_sql(
            &session,
            "INSERT INTO json_index_object_writes (id, doc) VALUES ('after', $1)",
            vec![Value::Json(serde_json::json!({"k": 2, "nested": [false]}))],
        );
        let insert_array = cassie.execute_sql(
            &session,
            "INSERT INTO json_index_object_writes (id, doc) VALUES ('array', $1)",
            vec![Value::Json(serde_json::json!(["entry", {"k": 3}]))],
        );
        let selected = cassie.execute_sql(
            &session,
            "SELECT id FROM json_index_object_writes WHERE doc = $1",
            vec![Value::Json(serde_json::json!({"nested": [false], "k": 2}))],
        );
        let selected_array = cassie.execute_sql(
            &session,
            "SELECT id FROM json_index_object_writes WHERE doc = $1",
            vec![Value::Json(serde_json::json!(["entry", {"k": 3}]))],
        );
        let equality_explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT id FROM json_index_object_writes WHERE doc = $1",
                vec![Value::Json(serde_json::json!({"nested": [false], "k": 2}))],
            )
            .expect("explain JSON equality");

        // Assert
        assert!(index.is_ok(), "create JSON index");
        assert!(insert.is_ok(), "insert JSON object after index");
        assert!(insert_array.is_ok(), "insert JSON array after index");
        assert_eq!(
            selected.expect("query indexed JSON object").rows(),
            vec![vec![Value::String("after".to_string())]]
        );
        assert_eq!(
            selected_array.expect("query indexed JSON array").rows(),
            vec![vec![Value::String("array".to_string())]]
        );
        let Value::String(plan) = &equality_explain.rows()[0][0] else {
            panic!("expected textual plan");
        };
        assert!(plan.contains("access_path=index_seek"));
        assert!(plan.contains("index=postgres.public.json_index_object_writes_doc_idx"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_enforce_json_unique_index_for_object_values() {
    // Arrange
    use_local_storage();
    let path = data_dir("json_unique_object_values");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE json_unique_object_values (id TEXT, doc JSON UNIQUE)",
                vec![],
            )
            .expect("create unique JSON table");

        // Act
        let first = cassie.execute_sql(
            &session,
            "INSERT INTO json_unique_object_values (id, doc) VALUES ('first', $1)",
            vec![Value::Json(serde_json::json!({"k": 1, "nested": [true]}))],
        );
        let duplicate = cassie.execute_sql(
            &session,
            "INSERT INTO json_unique_object_values (id, doc) VALUES ('duplicate', $1)",
            vec![Value::Json(serde_json::json!({"nested": [true], "k": 1}))],
        );
        let first_array = cassie.execute_sql(
            &session,
            "INSERT INTO json_unique_object_values (id, doc) VALUES ('array', $1)",
            vec![Value::Json(serde_json::json!(["same", {"k": 1}]))],
        );
        let duplicate_array = cassie.execute_sql(
            &session,
            "INSERT INTO json_unique_object_values (id, doc) VALUES ('duplicate_array', $1)",
            vec![Value::Json(serde_json::json!(["same", {"k": 1}]))],
        );

        // Assert
        assert!(first.is_ok(), "insert first JSON object");
        assert!(duplicate.is_err(), "duplicate JSON object should conflict");
        assert!(first_array.is_ok(), "insert first JSON array");
        assert!(
            duplicate_array.is_err(),
            "duplicate JSON array should conflict"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_backfill_native_array_index_without_poisoning_later_ddl() {
    // Arrange
    use_local_storage();
    let path = data_dir("native_array_index_publication");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
            cassie.startup().expect("startup");
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE native_array_index_publication (id TEXT, vals INT[])",
                    vec![],
                )
                .expect("create array table");
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO native_array_index_publication (id, vals) VALUES ('seed', $1)",
                    vec![Value::Json(serde_json::json!([2]))],
                )
                .expect("insert array before index");

            // Act
            let array_index = cassie.execute_sql(
                &session,
                "CREATE INDEX native_array_index_publication_vals_idx ON native_array_index_publication (vals)",
                vec![],
            );
            let unrelated_table = cassie.execute_sql(
                &session,
                "CREATE TABLE native_array_index_followup (id TEXT, value TEXT)",
                vec![],
            );
            let unrelated_index = cassie.execute_sql(
                &session,
                "CREATE INDEX native_array_index_followup_value_idx ON native_array_index_followup (value)",
                vec![],
            );

            // Assert
            assert!(array_index.is_ok(), "backfill array index");
            assert!(unrelated_table.is_ok(), "create follow-up table");
            assert!(unrelated_index.is_ok(), "create follow-up index");
            assert!(cassie.catalog.get_index(
                "native_array_index_publication",
                "native_array_index_publication_vals_idx"
            ).is_some());

            let _ = std::fs::remove_dir_all(path);
        });
}

fn create_native_array_index_tables(cassie: &Cassie, session: &cassie::app::CassieSession) {
    cassie
        .execute_sql(
            session,
            "CREATE TABLE native_array_primary (vals TEXT[] PRIMARY KEY, note TEXT)",
            vec![],
        )
        .expect("create array primary-key table");
    cassie
        .execute_sql(
            session,
            "CREATE TABLE native_array_unique (id TEXT, vals INT[] UNIQUE)",
            vec![],
        )
        .expect("create array unique table");
    cassie
        .execute_sql(
            session,
            "CREATE TABLE native_array_secondary (id TEXT, vals INT[])",
            vec![],
        )
        .expect("create array secondary-index table");
    cassie
        .execute_sql(
            session,
            "CREATE INDEX native_array_secondary_vals_idx ON native_array_secondary (vals)",
            vec![],
        )
        .expect("create array secondary index");
}

#[test]
fn should_enforce_native_array_primary_key() {
    // Arrange
    use_local_storage();
    let path = data_dir("native_array_primary_key");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        create_native_array_index_tables(&cassie, &session);

        // Act
        let first = cassie.execute_sql(
            &session,
            "INSERT INTO native_array_primary (vals, note) VALUES ($1, 'first')",
            vec![Value::Json(serde_json::json!(["z"]))],
        );
        let duplicate = cassie.execute_sql(
            &session,
            "INSERT INTO native_array_primary (vals, note) VALUES ($1, 'duplicate')",
            vec![Value::Json(serde_json::json!(["z"]))],
        );

        // Assert
        assert!(first.is_ok(), "insert primary array");
        assert!(
            duplicate.is_err(),
            "duplicate primary array should conflict"
        );
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_enforce_native_array_unique_constraint() {
    // Arrange
    use_local_storage();
    let path = data_dir("native_array_unique_constraint");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        create_native_array_index_tables(&cassie, &session);

        // Act
        let first = cassie.execute_sql(
            &session,
            "INSERT INTO native_array_unique (id, vals) VALUES ('first', $1)",
            vec![Value::Json(serde_json::json!([1, 2]))],
        );
        let duplicate = cassie.execute_sql(
            &session,
            "INSERT INTO native_array_unique (id, vals) VALUES ('duplicate', $1)",
            vec![Value::Json(serde_json::json!([1, 2]))],
        );

        // Assert
        assert!(first.is_ok(), "insert unique array");
        assert!(duplicate.is_err(), "duplicate unique array should conflict");
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_write_native_array_to_secondary_index() {
    // Arrange
    use_local_storage();
    let path = data_dir("native_array_secondary_index");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        create_native_array_index_tables(&cassie, &session);

        // Act
        let inserted = cassie.execute_sql(
            &session,
            "INSERT INTO native_array_secondary (id, vals) VALUES ('first', $1)",
            vec![Value::Json(serde_json::json!([7]))],
        );

        // Assert
        assert!(inserted.is_ok(), "insert indexed array");
        let _ = std::fs::remove_dir_all(path);
    });
}

fn create_json_range_order_fixture(cassie: &Cassie, session: &cassie::app::CassieSession) {
    cassie
        .execute_sql(
            session,
            "CREATE TABLE json_index_range_order (id TEXT, doc JSON NOT NULL)",
            vec![],
        )
        .expect("create indexed table");
    cassie
        .execute_sql(
            session,
            "CREATE TABLE json_index_range_order_scan (id TEXT, doc JSON NOT NULL)",
            vec![],
        )
        .expect("create scan comparison table");
    for (id, doc) in [
        ("string", serde_json::json!("a")),
        ("number", serde_json::json!(999)),
        ("float", serde_json::json!(1.5)),
        ("array", serde_json::json!([0])),
        ("boolean", serde_json::json!(true)),
        ("object", serde_json::json!({"k": 1})),
    ] {
        for table in ["json_index_range_order", "json_index_range_order_scan"] {
            let value = if doc.is_string() {
                Value::String(doc.to_string())
            } else {
                Value::Json(doc.clone())
            };
            cassie
                .execute_sql(
                    session,
                    &format!("INSERT INTO {table} (id, doc) VALUES ($1, $2)"),
                    vec![Value::String(id.to_string()), value],
                )
                .expect("insert JSON value");
        }
    }
    for number in 0..128 {
        for table in ["json_index_range_order", "json_index_range_order_scan"] {
            cassie
                .execute_sql(
                    session,
                    &format!("INSERT INTO {table} (id, doc) VALUES ($1, $2)"),
                    vec![
                        Value::String(format!("number_{number:03}")),
                        Value::Json(serde_json::json!(number)),
                    ],
                )
                .expect("insert JSON numeric value");
        }
    }
    cassie
        .execute_sql(
            session,
            "CREATE INDEX json_index_range_order_doc_idx ON json_index_range_order (doc)",
            vec![],
        )
        .expect("create index over existing JSON values");
}

#[test]
fn should_preserve_json_range_results_with_an_index() {
    // Arrange
    use_local_storage();
    let path = data_dir("json_index_range_results");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        create_json_range_order_fixture(&cassie, &session);

        // Act
        let range = cassie
            .execute_sql(
                &session,
                "SELECT id FROM json_index_range_order WHERE doc > $1",
                vec![Value::Json(serde_json::json!(1.5))],
            )
            .expect("query JSON range");
        let range_scan = cassie
            .execute_sql(
                &session,
                "SELECT id FROM json_index_range_order_scan WHERE doc > $1",
                vec![Value::Json(serde_json::json!(1.5))],
            )
            .expect("query JSON range without index");
        let range_explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT id FROM json_index_range_order WHERE doc > $1",
                vec![Value::Json(serde_json::json!(1.5))],
            )
            .expect("explain JSON range");

        // Assert
        let range_ids = range
            .rows()
            .iter()
            .filter_map(|row| match row.first() {
                Some(Value::String(id)) => Some(id.clone()),
                _ => None,
            })
            .collect::<std::collections::BTreeSet<_>>();
        let range_scan_ids = range_scan
            .rows()
            .iter()
            .filter_map(|row| match row.first() {
                Some(Value::String(id)) => Some(id.clone()),
                _ => None,
            })
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(range_ids, range_scan_ids);
        let Value::String(plan) = &range_explain.rows()[0][0] else {
            panic!("expected textual JSON range plan");
        };
        assert!(plan.contains("access_path=range_scan"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_preserve_json_order_results_with_an_index() {
    // Arrange
    use_local_storage();
    let path = data_dir("json_index_order_results");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        create_json_range_order_fixture(&cassie, &session);

        // Act
        let ordered = cassie
            .execute_sql(
                &session,
                "SELECT id FROM json_index_range_order ORDER BY doc LIMIT 5",
                vec![],
            )
            .expect("order JSON values");
        let ordered_scan = cassie
            .execute_sql(
                &session,
                "SELECT id FROM json_index_range_order_scan ORDER BY doc LIMIT 5",
                vec![],
            )
            .expect("order JSON values without index");
        let order_explain = cassie
            .execute_sql(
                &session,
                "EXPLAIN SELECT id FROM json_index_range_order ORDER BY doc LIMIT 5",
                vec![],
            )
            .expect("explain JSON order");

        // Assert
        let Value::String(plan) = &order_explain.rows()[0][0] else {
            panic!("expected textual JSON order plan");
        };
        assert!(plan.contains("access_path=ordered_bounded_scan"));
        assert_eq!(ordered.rows(), ordered_scan.rows());

        let _ = std::fs::remove_dir_all(path);
    });
}
