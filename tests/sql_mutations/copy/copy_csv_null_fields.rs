use cassie::app::Cassie;
use cassie::sql::ast::{CopyFormat, CopyStatement};
use cassie::types::Value;

use super::support_sql as support;
use support::{data_dir, use_local_storage};

#[test]
fn should_copy_unquoted_empty_csv_fields_as_null() {
    // Arrange
    use_local_storage();
    let path = data_dir("copy-csv-empty-null");
    let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE copy_csv_null_rows (id INT, flag BOOLEAN, note TEXT)",
            vec![],
        )
        .expect("create table");
    let statement = CopyStatement {
        table: "copy_csv_null_rows".to_string(),
        columns: vec!["id".to_string(), "flag".to_string(), "note".to_string()],
        format: CopyFormat::Csv,
        header: false,
    };

    // Act
    let copied = cassie.copy_from_csv_stdin(&session, &statement, b"20,t,\"\"\n21,,\n22,f,kept\n");

    // Assert
    assert_eq!(copied.expect("PostgreSQL CSV NULLs should load"), 3);
    let rows = cassie
        .execute_sql(
            &session,
            "SELECT id, flag IS NULL, note IS NULL, note FROM copy_csv_null_rows ORDER BY id",
            vec![],
        )
        .expect("read copied rows")
        .rows()
        .to_vec();
    assert_eq!(
        rows,
        vec![
            vec![
                Value::Int64(20),
                Value::Bool(false),
                Value::Bool(false),
                Value::String(String::new()),
            ],
            vec![
                Value::Int64(21),
                Value::Bool(true),
                Value::Bool(true),
                Value::Null,
            ],
            vec![
                Value::Int64(22),
                Value::Bool(false),
                Value::Bool(false),
                Value::String("kept".into()),
            ],
        ]
    );
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);
}
