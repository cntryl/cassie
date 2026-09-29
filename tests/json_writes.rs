#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;

use cassie::app::Cassie;
use cassie::sql::ast::{CopyFormat, CopyStatement};
use cassie::types::Value;
use support_sql::{data_dir, use_local_storage};

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
fn should_reject_malformed_json_text_writes() {
    // Arrange
    use_local_storage();
    let path = data_dir("malformed_json_text_insert");
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
                "CREATE TABLE malformed_json_writes (id TEXT, doc JSON)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO malformed_json_writes (id, doc) VALUES ('valid', '{\"a\":1}')",
                vec![],
            )
            .expect("insert valid JSON");

        // Act
        let insert_result = cassie.execute_sql(
            &session,
            "INSERT INTO malformed_json_writes (id, doc) VALUES ('bad', '{not json')",
            vec![],
        );
        let update_result = cassie.execute_sql(
            &session,
            "UPDATE malformed_json_writes SET doc = '{not json' WHERE id = 'valid'",
            vec![],
        );
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT id, doc FROM malformed_json_writes",
                vec![],
            )
            .expect("select rows");

        // Assert
        assert!(insert_result.is_err());
        assert!(update_result.is_err());
        assert_eq!(
            selected.rows,
            vec![vec![
                Value::String("valid".to_string()),
                Value::Json(serde_json::json!({"a": 1}))
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
