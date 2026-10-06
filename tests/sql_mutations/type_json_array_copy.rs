use super::support_sql_fixture::{sql_fixture, SqlFixture};
use cassie::app::CassieError;
use cassie::sql::ast::{CopyFormat, CopyStatement};
use cassie::types::Value;

fn fixture(label: &str) -> SqlFixture {
    let fixture = sql_fixture(label, &[]);
    fixture
        .cassie
        .startup()
        .expect("bootstrap canonical catalog");
    fixture
        .execute("CREATE TABLE json_array_copy (id INT, docs JSON[])")
        .expect("create JSON array table");
    fixture
}

fn statement() -> CopyStatement {
    CopyStatement {
        table: "json_array_copy".to_string(),
        columns: vec!["id".to_string(), "docs".to_string()],
        format: CopyFormat::Csv,
        header: false,
    }
}

fn csv_row(id: u8, document: &str) -> String {
    format!("{id},\"{}\"\n", document.replace('"', "\"\""))
}

#[test]
fn should_reject_copy_json_document_null_elements_atomically() {
    // Arrange
    let fixture = fixture("type-copy-json-document-null-atomic");
    fixture
        .cassie
        .execute_sql(
            &fixture.session,
            "INSERT INTO json_array_copy (id, docs) VALUES ($1, $2)",
            vec![
                Value::Int64(0),
                Value::Json(serde_json::json!([{"seed": true}])),
            ],
        )
        .expect("seed prior committed row");
    let statement = statement();
    let payload = csv_row(1, "[[null],null]") + &csv_row(2, r#"{"null"}"#);

    // Act
    let result =
        fixture
            .cassie
            .copy_from_csv_stdin(&fixture.session, &statement, payload.as_bytes());

    // Assert
    assert!(matches!(result, Err(CassieError::Unsupported(_))));
    assert_eq!(
        fixture.rows("SELECT id, docs FROM json_array_copy ORDER BY id"),
        vec![vec![
            Value::Int64(0),
            Value::Json(serde_json::json!([{"seed": true}]))
        ]]
    );
}

#[test]
fn should_preserve_copy_malformed_json_array_parse_errors() {
    // Arrange
    let fixture = fixture("type-copy-json-array-malformed-priority");
    let statement = statement();
    let payload = csv_row(1, "[[null],null]") + &csv_row(2, r#"{"null","broken"}"#);

    // Act
    let result =
        fixture
            .cassie
            .copy_from_csv_stdin(&fixture.session, &statement, payload.as_bytes());

    // Assert
    assert!(matches!(result, Err(CassieError::Parse(_))));
    assert_eq!(
        fixture.rows("SELECT id FROM json_array_copy"),
        Vec::<Vec<Value>>::new()
    );
}

#[test]
fn should_preserve_copy_json_array_null_slot_origin() {
    // Arrange
    let fixture = fixture("type-copy-json-array-null-origin");
    let statement = statement();
    let payload = csv_row(1, "{NULL}")
        + &csv_row(2, "{[null]}")
        + &csv_row(3, r#"["null"]"#)
        + &csv_row(4, "[null]");

    // Act
    let result =
        fixture
            .cassie
            .copy_from_csv_stdin(&fixture.session, &statement, payload.as_bytes());

    // Assert
    assert_eq!(
        result.expect("COPY retains supported JSON array meanings"),
        4
    );
    assert_eq!(
        fixture.rows("SELECT docs FROM json_array_copy ORDER BY id"),
        vec![
            vec![Value::Json(serde_json::json!([null]))],
            vec![Value::Json(serde_json::json!([[null]]))],
            vec![Value::Json(serde_json::json!(["null"]))],
            vec![Value::Json(serde_json::json!([null]))],
        ]
    );
}
