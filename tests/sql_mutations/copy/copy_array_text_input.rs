use cassie::app::Cassie;
use cassie::sql::ast::{CopyFormat, CopyStatement};
use cassie::types::Value;

use super::support_sql as support;
use support::{data_dir, use_local_storage};

#[test]
fn should_copy_postgres_array_text_into_array_columns() {
    // Arrange
    use_local_storage();
    let path = data_dir("copy-postgres-array-text");
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE copy_array_text_rows (id TEXT, tags TEXT[])",
            vec![],
        )
        .expect("create array table");
    let statement = CopyStatement {
        table: "copy_array_text_rows".to_string(),
        columns: vec!["id".to_string(), "tags".to_string()],
        format: CopyFormat::Csv,
        header: false,
    };

    // Act
    let copied = cassie.copy_from_csv_stdin(&session, &statement, b"empty,{}\nitems,\"{a,b}\"\n");

    // Assert
    assert_eq!(copied.expect("PostgreSQL CSV arrays should load"), 2);
    let rows = cassie
        .execute_sql(
            &session,
            "SELECT id, tags FROM copy_array_text_rows ORDER BY id",
            vec![],
        )
        .expect("read copied arrays")
        .rows()
        .to_vec();
    assert_eq!(
        rows,
        vec![
            vec![
                Value::String("empty".into()),
                Value::Json(serde_json::json!([]))
            ],
            vec![
                Value::String("items".into()),
                Value::Json(serde_json::json!(["a", "b"])),
            ],
        ]
    );
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}
