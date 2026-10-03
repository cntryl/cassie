#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;

use cassie::app::{Cassie, CassieSession};
use cassie::sql::ast::{CopyFormat, CopyStatement};
use cassie::types::Value;
use support_sql::{data_dir, use_local_storage};

fn with_local_json_database(name: &str, test: impl FnOnce(&Cassie, &CassieSession)) {
    use_local_storage();
    let path = data_dir(name);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        test(&cassie, &session);
        drop(session);
        drop(cassie);
    });
    let _ = std::fs::remove_dir_all(path);
}

fn seed_valid_json_row(cassie: &Cassie, session: &CassieSession, table: &str) {
    cassie
        .execute_sql(
            session,
            &format!("CREATE TABLE {table} (id TEXT, doc JSON)"),
            vec![],
        )
        .expect("create table");
    cassie
        .execute_sql(
            session,
            &format!("INSERT INTO {table} (id, doc) VALUES ('valid', '{{\"a\":1}}')"),
            vec![],
        )
        .expect("insert valid JSON");
}

#[test]
fn should_normalize_json_text_writes() {
    // Arrange
    use_local_storage();
    let path = data_dir("json_text_insert_update");
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
                "CREATE TABLE json_text_writes (id TEXT PRIMARY KEY, doc JSON)",
                vec![],
            )
            .expect("create table");

        // Act
        cassie
            .execute_sql(
                &session,
                "INSERT INTO json_text_writes (id, doc) VALUES ('insert', '{\"a\":1}')",
                vec![],
            )
            .expect("insert JSON text");
        cassie
            .copy_from_csv_stdin(
                &session,
                &CopyStatement {
                    table: "json_text_writes".to_string(),
                    columns: vec!["id".to_string(), "doc".to_string()],
                    format: CopyFormat::Csv,
                    header: false,
                },
                b"copy,\"{\"\"a\"\":1}\"\n",
            )
            .expect("copy JSON text");
        let distinct = cassie
            .execute_sql(
                &session,
                "SELECT DISTINCT doc FROM json_text_writes",
                vec![],
            )
            .expect("select distinct JSON value");
        cassie
            .execute_sql(
                &session,
                "UPDATE json_text_writes SET doc = '{\"k\":2}' WHERE id = 'insert'",
                vec![],
            )
            .expect("update JSON text");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO json_text_writes (id, doc) VALUES ('insert', '{\"r\":3}') ON CONFLICT (id) DO UPDATE SET doc = '{\"c\":3}'",
                vec![],
            )
            .expect("upsert JSON text");
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT doc FROM json_text_writes WHERE id = 'insert'",
                vec![],
            )
            .expect("select JSON value");

        // Assert
        assert_eq!(
            distinct.rows,
            vec![vec![Value::Json(serde_json::json!({"a": 1}))]]
        );
        assert_eq!(
            selected.rows,
            vec![vec![Value::Json(serde_json::json!({"c": 3}))]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_malformed_json_text_insert() {
    // Arrange
    with_local_json_database("malformed_json_text_insert", |cassie, session| {
        seed_valid_json_row(cassie, session, "malformed_json_insert");

        // Act
        let insert_result = cassie.execute_sql(
            session,
            "INSERT INTO malformed_json_insert (id, doc) VALUES ('bad', 'not json')",
            vec![],
        );

        // Assert
        assert!(insert_result.is_err());
    });
}

#[test]
fn should_reject_malformed_json_text_update() {
    // Arrange
    with_local_json_database("malformed_json_text_update", |cassie, session| {
        seed_valid_json_row(cassie, session, "malformed_json_update");

        // Act
        let update_result = cassie.execute_sql(
            session,
            "UPDATE malformed_json_update SET doc = 'not json' WHERE id = 'valid'",
            vec![],
        );
        // Assert
        assert!(update_result.is_err());
    });
}

#[test]
fn should_distinguish_json_document_null_from_sql_null() {
    // Arrange
    with_local_json_database("json_document_null", |cassie, session| {
        cassie
            .execute_sql(
                session,
                "CREATE TABLE json_document_nulls (id TEXT PRIMARY KEY, doc JSON)",
                vec![],
            )
            .expect("create table");

        // Act
        cassie
            .execute_sql(
                session,
                "INSERT INTO json_document_nulls (id, doc) VALUES ('literal-json-null', 'null'), ('literal-sql-null', NULL)",
                vec![],
            )
            .expect("insert JSON and SQL null literals");
        cassie
            .execute_sql(
                session,
                "INSERT INTO json_document_nulls (id, doc) VALUES ('bound-json-null', $1), ('bound-sql-null', $2)",
                vec![Value::Json(serde_json::Value::Null), Value::Null],
            )
            .expect("insert bound JSON and SQL nulls");
        cassie
            .copy_from_csv_stdin(
                session,
                &CopyStatement {
                    table: "json_document_nulls".to_string(),
                    columns: vec!["id".to_string(), "doc".to_string()],
                    format: CopyFormat::Csv,
                    header: false,
                },
                b"copy-json-null,null\ncopy-sql-null,\\N\n",
            )
            .expect("copy JSON and SQL null values");
        let rows = cassie
            .execute_sql(
                session,
                "SELECT id, doc, doc IS NULL FROM json_document_nulls ORDER BY id",
                vec![],
            )
            .expect("select JSON null distinctions");
        let count = cassie
            .execute_sql(
                session,
                "SELECT COUNT(doc) FROM json_document_nulls",
                vec![],
            )
            .expect("count non-SQL-null JSON values");

        // Assert
        assert_eq!(
            rows.rows,
            vec![
                vec![
                    Value::String("bound-json-null".to_string()),
                    Value::Json(serde_json::Value::Null),
                    Value::Bool(false),
                ],
                vec![
                    Value::String("bound-sql-null".to_string()),
                    Value::Null,
                    Value::Bool(true),
                ],
                vec![
                    Value::String("copy-json-null".to_string()),
                    Value::Json(serde_json::Value::Null),
                    Value::Bool(false),
                ],
                vec![
                    Value::String("copy-sql-null".to_string()),
                    Value::Null,
                    Value::Bool(true),
                ],
                vec![
                    Value::String("literal-json-null".to_string()),
                    Value::Json(serde_json::Value::Null),
                    Value::Bool(false),
                ],
                vec![
                    Value::String("literal-sql-null".to_string()),
                    Value::Null,
                    Value::Bool(true),
                ],
            ]
        );
        assert_eq!(count.rows, vec![vec![Value::Int64(3)]]);
    });
}

#[test]
fn should_allow_json_null_in_required_json_field() {
    with_local_json_database("json_null_required_rest", |cassie, session| {
        // Arrange
        cassie
            .execute_sql(
                session,
                "CREATE TABLE json_required (id TEXT PRIMARY KEY, doc JSON NOT NULL)",
                vec![],
            )
            .expect("create required JSON table");

        // Act
        let json_null = cassie.execute_sql(
            session,
            "INSERT INTO json_required (id, doc) VALUES ('json-null', 'null')",
            vec![],
        );
        let sql_null = cassie.execute_sql(
            session,
            "INSERT INTO json_required (id, doc) VALUES ('sql-null', NULL)",
            vec![],
        );

        // Assert
        assert!(json_null.is_ok());
        assert!(sql_null.is_err());
    });
}

#[test]
fn should_preserve_json_null_in_rest_document() {
    with_local_json_database("json_null_rest_document", |cassie, session| {
        // Arrange
        cassie
            .execute_sql(
                session,
                "CREATE TABLE json_rest_document (id TEXT PRIMARY KEY, doc JSON NOT NULL)",
                vec![],
            )
            .expect("create REST JSON table");

        // Act
        let rest_created = cassie::rest::documents::create(
            cassie,
            "json_rest_document",
            br#"{"id":"rest-json-null","doc":null}"#,
        )
        .expect("create JSON-null document through REST");
        let rest_id = rest_created["id"].as_str().expect("REST document identity");
        let rest_document = cassie::rest::documents::get(cassie, "json_rest_document", rest_id)
            .expect("read JSON-null REST document");

        // Assert
        assert_eq!(rest_document.get("doc"), Some(&serde_json::Value::Null));
    });
}

#[test]
fn should_omit_sql_null_fields_from_raw_documents() {
    with_local_json_database("json_null_raw_payload", |cassie, session| {
        // Arrange
        cassie
            .execute_sql(
                session,
                "CREATE TABLE json_null_raw (id TEXT PRIMARY KEY, doc JSON)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                session,
                "INSERT INTO json_null_raw (id, doc) VALUES ('json-null', 'null'), ('sql-null', NULL)",
                vec![],
            )
            .expect("insert both null values");

        // Act
        let identities = cassie
            .execute_sql(
                session,
                "SELECT id, _id FROM json_null_raw ORDER BY id",
                vec![],
            )
            .expect("read row identities");
        let Value::String(json_id) = &identities.rows[0][1] else {
            panic!("row identity should be text");
        };
        let Value::String(sql_id) = &identities.rows[1][1] else {
            panic!("row identity should be text");
        };
        let json_document = cassie::rest::documents::get(cassie, "json_null_raw", json_id)
            .expect("read JSON-null raw document");
        let sql_document = cassie::rest::documents::get(cassie, "json_null_raw", sql_id)
            .expect("read SQL-null raw document");

        // Assert
        assert_eq!(json_document.get("doc"), Some(&serde_json::Value::Null));
        assert!(sql_document.get("doc").is_none());
    });
}

#[test]
fn should_return_json_null_from_insert_returning() {
    with_local_json_database("json_null_insert_returning", |cassie, session| {
        // Arrange
        cassie
            .execute_sql(
                session,
                "CREATE TABLE json_null_returning (id TEXT PRIMARY KEY, doc JSON)",
                vec![],
            )
            .expect("create table");

        // Act
        let returned = cassie
            .execute_sql(
                session,
                "INSERT INTO json_null_returning (id, doc) VALUES ('null', 'null') RETURNING doc",
                vec![],
            )
            .expect("return inserted JSON null");

        // Assert
        assert_eq!(
            returned.rows,
            vec![vec![Value::Json(serde_json::Value::Null)]]
        );
    });
}

#[test]
fn should_preserve_json_null_in_upsert() {
    with_local_json_database("json_null_upsert", |cassie, session| {
        // Arrange
        cassie
            .execute_sql(
                session,
                "CREATE TABLE json_null_upsert (id TEXT PRIMARY KEY, doc JSON)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                session,
                "INSERT INTO json_null_upsert (id, doc) VALUES ('row', '{}')",
                vec![],
            )
            .expect("insert initial row");

        // Act
        cassie
            .execute_sql(
                session,
                "INSERT INTO json_null_upsert (id, doc) VALUES ('row', 'null') ON CONFLICT (id) DO UPDATE SET doc = excluded.doc",
                vec![],
            )
            .expect("upsert JSON document null");
        let row = cassie
            .execute_sql(session, "SELECT doc FROM json_null_upsert", vec![])
            .expect("read upserted value");

        // Assert
        assert_eq!(row.rows, vec![vec![Value::Json(serde_json::Value::Null)]]);
    });
}

#[test]
fn should_distinguish_null_values_after_update() {
    with_local_json_database("json_null_update", |cassie, session| {
        // Arrange
        cassie
            .execute_sql(
                session,
                "CREATE TABLE json_null_update (id TEXT PRIMARY KEY, doc JSON)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                session,
                "INSERT INTO json_null_update (id, doc) VALUES ('to-sql-null', 'null'), ('to-json-null', NULL)",
                vec![],
            )
            .expect("insert initial values");

        // Act
        cassie
            .execute_sql(
                session,
                "UPDATE json_null_update SET doc = NULL WHERE id = 'to-sql-null'",
                vec![],
            )
            .expect("update JSON null to SQL NULL");
        cassie
            .execute_sql(
                session,
                "UPDATE json_null_update SET doc = 'null' WHERE id = 'to-json-null'",
                vec![],
            )
            .expect("update SQL NULL to JSON null");
        let rows = cassie
            .execute_sql(
                session,
                "SELECT id, doc, doc IS NULL FROM json_null_update ORDER BY id",
                vec![],
            )
            .expect("select updated values");

        // Assert
        assert_eq!(
            rows.rows,
            vec![
                vec![
                    Value::String("to-json-null".to_string()),
                    Value::Json(serde_json::Value::Null),
                    Value::Bool(false),
                ],
                vec![
                    Value::String("to-sql-null".to_string()),
                    Value::Null,
                    Value::Bool(true),
                ],
            ]
        );
    });
}

#[test]
fn should_preserve_json_null_distinction_after_restart() {
    // Arrange
    use_local_storage();
    let path = data_dir("json_null_restart");
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
                "CREATE TABLE json_null_restart (id TEXT PRIMARY KEY, doc JSON)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO json_null_restart (id, doc) VALUES ('json-null', 'null'), ('sql-null', NULL)",
                vec![],
            )
            .expect("insert both null values");
        drop(session);
        drop(cassie);

        // Act
        let reopened = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
        reopened.startup().expect("restart Cassie");
        let reopened_session = reopened.create_session("tester", None);
        let rows = reopened
            .execute_sql(
                &reopened_session,
                "SELECT id, doc, doc IS NULL FROM json_null_restart ORDER BY id",
                vec![],
            )
            .expect("read values after restart");

        // Assert
        assert_eq!(
            rows.rows,
            vec![
                vec![Value::String("json-null".to_string()), Value::Json(serde_json::Value::Null), Value::Bool(false)],
                vec![Value::String("sql-null".to_string()), Value::Null, Value::Bool(true)],
            ]
        );
        drop(reopened_session);
        drop(reopened);
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_preserve_json_null_in_column_storage_paths() {
    // Arrange
    use_local_storage();
    let path = data_dir("json_null_column_storage");
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
                "CREATE TABLE json_null_column_store (id TEXT PRIMARY KEY, doc JSON) WITH (storage = column_store)",
                vec![],
            )
            .expect("create column store table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO json_null_column_store (id, doc) VALUES ('document-null', 'null'), ('sql-null', NULL)",
                vec![],
            )
            .expect("insert null values into column store");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE json_null_indexed (id TEXT PRIMARY KEY, doc JSON)",
                vec![],
            )
            .expect("create indexed table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO json_null_indexed (id, doc) VALUES ('document-null', 'null'), ('sql-null', NULL)",
                vec![],
            )
            .expect("insert null values before index creation");
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX json_null_indexed_column_idx ON json_null_indexed USING column (id, doc) WITH (segment_size = 1)",
                vec![],
            )
            .expect("create covering column index");

        // Act
        let column_store_rows = cassie
            .execute_sql(
                &session,
                "SELECT id, doc, doc IS NULL FROM json_null_column_store ORDER BY id",
                vec![],
            )
            .expect("read column store null distinctions");
        let indexed_rows = cassie
            .execute_sql(
                &session,
                "SELECT id, doc, doc IS NULL FROM json_null_indexed ORDER BY id",
                vec![],
            )
            .expect("read indexed null distinctions");
        let indexed_null_filter = cassie
            .execute_sql(
                &session,
                "SELECT id FROM json_null_indexed WHERE doc IS NULL ORDER BY id",
                vec![],
            )
            .expect("filter indexed rows by SQL NULL");
        let indexed_json_count = cassie
            .execute_sql(
                &session,
                "SELECT COUNT(doc) FROM json_null_indexed",
                vec![],
            )
            .expect("count JSON document null through indexed aggregate fallback");

        // Assert
        let expected_rows = vec![
            vec![
                Value::String("document-null".to_string()),
                Value::Json(serde_json::Value::Null),
                Value::Bool(false),
            ],
            vec![
                Value::String("sql-null".to_string()),
                Value::Null,
                Value::Bool(true),
            ],
        ];
        assert_eq!(column_store_rows.rows, expected_rows);
        assert_eq!(indexed_rows.rows, expected_rows);
        assert_eq!(
            indexed_null_filter.rows,
            vec![vec![Value::String("sql-null".to_string())]]
        );
        assert_eq!(indexed_json_count.rows, vec![vec![Value::Int64(1)]]);

        drop(session);
        drop(cassie);
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_enforce_uniqueness_for_json_null_but_allow_multiple_sql_nulls() {
    with_local_json_database("json_null_unique_values", |cassie, session| {
        // Arrange
        cassie
            .execute_sql(
                session,
                "CREATE TABLE json_null_unique_values (id TEXT PRIMARY KEY, doc JSON)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                session,
                "CREATE UNIQUE INDEX json_null_unique_values_doc_idx ON json_null_unique_values (doc)",
                vec![],
            )
            .expect("create unique JSON index");
        cassie
            .execute_sql(
                session,
                "INSERT INTO json_null_unique_values (id, doc) VALUES ('first-json-null', 'null')",
                vec![],
            )
            .expect("insert first JSON null");

        // Act
        let duplicate_json_null = cassie.execute_sql(
            session,
            "INSERT INTO json_null_unique_values (id, doc) VALUES ('second-json-null', 'null')",
            vec![],
        );
        let upsert_json_null = cassie.execute_sql(
            session,
            "INSERT INTO json_null_unique_values (id, doc) VALUES ('replacement-json-null', 'null') ON CONFLICT (doc) DO UPDATE SET id = excluded.id",
            vec![],
        );
        let upserted_json_null = cassie
            .execute_sql(
                session,
                "SELECT id, doc FROM json_null_unique_values WHERE id = 'replacement-json-null'",
                vec![],
            )
            .expect("read upserted unique JSON null");
        let first_sql_null = cassie.execute_sql(
            session,
            "INSERT INTO json_null_unique_values (id, doc) VALUES ('first-sql-null', NULL)",
            vec![],
        );
        let second_sql_null = cassie.execute_sql(
            session,
            "INSERT INTO json_null_unique_values (id, doc) VALUES ('second-sql-null', NULL)",
            vec![],
        );

        // Assert
        assert!(duplicate_json_null.is_err());
        assert!(upsert_json_null.is_ok(), "{upsert_json_null:?}");
        assert_eq!(
            upserted_json_null.rows,
            vec![vec![
                Value::String("replacement-json-null".to_string()),
                Value::Json(serde_json::Value::Null),
            ]]
        );
        assert!(first_sql_null.is_ok());
        assert!(second_sql_null.is_ok());
    });
}

#[test]
fn should_evaluate_partial_unique_predicates_with_json_null_semantics() {
    with_local_json_database("json_null_partial_unique", |cassie, session| {
        // Arrange
        cassie
            .execute_sql(
                session,
                "CREATE TABLE json_null_partial_unique (label TEXT, doc JSON)",
                vec![],
            )
            .expect("create partial unique table");
        cassie
            .execute_sql(
                session,
                "CREATE UNIQUE INDEX json_null_partial_unique_idx ON json_null_partial_unique (label) WHERE doc IS NULL",
                vec![],
            )
            .expect("create partial unique index");

        // Act
        let first_json_null = cassie.execute_sql(
            session,
            "INSERT INTO json_null_partial_unique (label, doc) VALUES ('same-label', 'null')",
            vec![],
        );
        let second_json_null = cassie.execute_sql(
            session,
            "INSERT INTO json_null_partial_unique (label, doc) VALUES ('same-label', 'null')",
            vec![],
        );
        let sql_null = cassie.execute_sql(
            session,
            "INSERT INTO json_null_partial_unique (label, doc) VALUES ('same-label', NULL)",
            vec![],
        );
        let duplicate_sql_null = cassie.execute_sql(
            session,
            "INSERT INTO json_null_partial_unique (label, doc) VALUES ('same-label', NULL)",
            vec![],
        );

        // Assert
        assert!(first_json_null.is_ok());
        assert!(second_json_null.is_ok());
        assert!(sql_null.is_ok());
        assert!(duplicate_sql_null.is_err());
    });
}

#[test]
fn should_check_json_null_foreign_keys_as_values() {
    with_local_json_database("json_null_foreign_key", |cassie, session| {
        // Arrange
        cassie
            .execute_sql(
                session,
                "CREATE TABLE json_null_fk_parent (doc JSON PRIMARY KEY)",
                vec![],
            )
            .expect("create parent table");
        cassie
            .execute_sql(
                session,
                "CREATE TABLE json_null_fk_child (id TEXT PRIMARY KEY, parent_doc JSON REFERENCES json_null_fk_parent(doc))",
                vec![],
            )
            .expect("create child table");

        // Act
        let missing_parent = cassie.execute_sql(
            session,
            "INSERT INTO json_null_fk_child (id, parent_doc) VALUES ('orphan', 'null')",
            vec![],
        );
        cassie
            .execute_sql(
                session,
                "INSERT INTO json_null_fk_parent (doc) VALUES ('null')",
                vec![],
            )
            .expect("insert JSON-null parent key");
        let matching_child = cassie.execute_sql(
            session,
            "INSERT INTO json_null_fk_child (id, parent_doc) VALUES ('child', 'null')",
            vec![],
        );
        let delete_referenced_parent = cassie.execute_sql(
            session,
            "DELETE FROM json_null_fk_parent WHERE doc IS NOT NULL",
            vec![],
        );

        // Assert
        assert!(missing_parent.is_err());
        assert!(matching_child.is_ok());
        assert!(delete_referenced_parent.is_err());
    });
}

#[test]
fn should_assign_sql_null_when_foreign_key_delete_uses_set_null() {
    with_local_json_database("json_null_fk_delete_setnull", |cassie, session| {
        // Arrange
        cassie
            .execute_sql(
                session,
                "CREATE TABLE fk_delete_parent (doc JSON PRIMARY KEY)",
                vec![],
            )
            .expect("create parent table");
        cassie
            .execute_sql(
                session,
                "CREATE TABLE fk_delete_child (id TEXT PRIMARY KEY, parent_doc JSON, FOREIGN KEY (parent_doc) REFERENCES fk_delete_parent(doc) ON DELETE SET NULL)",
                vec![],
            )
            .expect("create child table");
        cassie
            .execute_sql(
                session,
                "INSERT INTO fk_delete_parent (doc) VALUES ('null')",
                vec![],
            )
            .expect("insert parent JSON null");
        cassie
            .execute_sql(
                session,
                "INSERT INTO fk_delete_child (id, parent_doc) VALUES ('child', 'null')",
                vec![],
            )
            .expect("insert child JSON null");

        // Act
        cassie
            .execute_sql(
                session,
                "DELETE FROM fk_delete_parent WHERE doc IS NOT NULL",
                vec![],
            )
            .expect("delete referenced parent");

        // Assert
        let child = cassie
            .execute_sql(
                session,
                "SELECT parent_doc, parent_doc IS NULL FROM fk_delete_child",
                vec![],
            )
            .expect("read cascaded SQL NULL");
        assert_eq!(child.rows, vec![vec![Value::Null, Value::Bool(true)]]);
    });
}

#[test]
fn should_assign_sql_null_when_foreign_key_update_uses_set_null() {
    with_local_json_database("json_null_fk_update_setnull", |cassie, session| {
        // Arrange
        cassie
            .execute_sql(
                session,
                "CREATE TABLE fk_update_parent (doc JSON PRIMARY KEY)",
                vec![],
            )
            .expect("create parent table");
        cassie
            .execute_sql(
                session,
                "CREATE TABLE fk_update_child (id TEXT PRIMARY KEY, parent_doc JSON, FOREIGN KEY (parent_doc) REFERENCES fk_update_parent(doc) ON UPDATE SET NULL)",
                vec![],
            )
            .expect("create child table");
        cassie
            .execute_sql(
                session,
                "INSERT INTO fk_update_parent (doc) VALUES ('null')",
                vec![],
            )
            .expect("insert parent JSON null");
        cassie
            .execute_sql(
                session,
                "INSERT INTO fk_update_child (id, parent_doc) VALUES ('child', 'null')",
                vec![],
            )
            .expect("insert child JSON null");

        // Act
        cassie
            .execute_sql(
                session,
                "UPDATE fk_update_parent SET doc = $1 WHERE doc IS NOT NULL",
                vec![Value::Json(serde_json::json!("replacement"))],
            )
            .expect("update referenced parent");

        // Assert
        let child = cassie
            .execute_sql(
                session,
                "SELECT parent_doc, parent_doc IS NULL FROM fk_update_child",
                vec![],
            )
            .expect("read set-null child");
        assert_eq!(child.rows, vec![vec![Value::Null, Value::Bool(true)]]);
    });
}

#[test]
fn should_cascade_sql_null_when_parent_json_null_becomes_sql_null() {
    with_local_json_database("json_null_fk_update_cascade", |cassie, session| {
        // Arrange
        cassie
            .execute_sql(
                session,
                "CREATE TABLE fk_cascade_parent (doc JSON UNIQUE)",
                vec![],
            )
            .expect("create parent table");
        cassie
            .execute_sql(
                session,
                "CREATE TABLE fk_cascade_child (id TEXT PRIMARY KEY, parent_doc JSON, FOREIGN KEY (parent_doc) REFERENCES fk_cascade_parent(doc) ON UPDATE CASCADE)",
                vec![],
            )
            .expect("create child table");
        cassie
            .execute_sql(
                session,
                "INSERT INTO fk_cascade_parent (doc) VALUES ('null')",
                vec![],
            )
            .expect("insert parent JSON null");
        cassie
            .execute_sql(
                session,
                "INSERT INTO fk_cascade_child (id, parent_doc) VALUES ('child', 'null')",
                vec![],
            )
            .expect("insert child JSON null");

        // Act
        cassie
            .execute_sql(
                session,
                "UPDATE fk_cascade_parent SET doc = NULL WHERE doc IS NOT NULL",
                vec![],
            )
            .expect("update parent key to SQL NULL");

        // Assert
        let child = cassie
            .execute_sql(
                session,
                "SELECT parent_doc, parent_doc IS NULL FROM fk_cascade_child",
                vec![],
            )
            .expect("read cascaded SQL NULL");
        assert_eq!(child.rows, vec![vec![Value::Null, Value::Bool(true)]]);
    });
}

#[test]
fn should_apply_check_constraints_to_json_null_values() {
    // Arrange
    use_local_storage();
    let path = data_dir("json_null_check_constraint");
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
                "CREATE TABLE json_null_check_values (id TEXT PRIMARY KEY, doc JSON CHECK (doc > 0))",
                vec![],
            )
            .expect("create checked JSON table");

        // Act
        let sql_null = cassie.execute_sql(
            &session,
            "INSERT INTO json_null_check_values (id, doc) VALUES ('sql-null', NULL)",
            vec![],
        );
        let json_null = cassie.execute_sql(
            &session,
            "INSERT INTO json_null_check_values (id, doc) VALUES ('json-null', 'null')",
            vec![],
        );

        // Assert
        assert!(sql_null.is_ok());
        assert!(json_null.is_err());

        drop(session);
        drop(cassie);
        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_type_json_null_inside_unique_expression_indexes() {
    // Arrange
    use_local_storage();
    let path = data_dir("json_null_expression_index");
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
                "CREATE TABLE json_null_expression_values (id TEXT PRIMARY KEY, doc JSON)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                &session,
                "CREATE UNIQUE INDEX json_null_expression_values_idx ON json_null_expression_values ((doc IS NULL))",
                vec![],
            )
            .expect("create unique expression index");

        // Act
        let json_null = cassie.execute_sql(
            &session,
            "INSERT INTO json_null_expression_values (id, doc) VALUES ('json-null', 'null')",
            vec![],
        );
        let sql_null = cassie.execute_sql(
            &session,
            "INSERT INTO json_null_expression_values (id, doc) VALUES ('sql-null', NULL)",
            vec![],
        );
        let duplicate_json_null = cassie.execute_sql(
            &session,
            "INSERT INTO json_null_expression_values (id, doc) VALUES ('duplicate-json-null', 'null')",
            vec![],
        );

        // Assert
        assert!(json_null.is_ok());
        assert!(sql_null.is_ok());
        assert!(duplicate_json_null.is_err());

        drop(session);
        drop(cassie);
        let _ = std::fs::remove_dir_all(path);
    });
}
