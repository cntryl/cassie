use super::support_sql_fixture::sql_fixture;
use cassie::app::CassieError;
use cassie::sql::ast::{CopyFormat, CopyStatement};
use cassie::types::Value;

#[test]
fn should_reject_copy_scalar_array_input_before_publication() {
    // Arrange
    let cases = [
        ("integer", "INT[]", "[7]", "[true]", serde_json::json!([7])),
        (
            "char",
            "CHAR(2)[]",
            r#"["éA  "]"#,
            r#"["éAB"]"#,
            serde_json::json!(["éA"]),
        ),
        (
            "varchar",
            "VARCHAR(2)[]",
            r#"["éA"]"#,
            r#"["éAB"]"#,
            serde_json::json!(["éA"]),
        ),
    ];
    for (label, sql_type, valid, invalid, expected) in cases {
        let fixture = sql_fixture(&format!("type-array-copy-input-{label}"), &[]);
        fixture.cassie.startup().expect("bootstrap");
        fixture
            .execute(&format!(
                "CREATE TABLE array_input_copy (id INT, v {sql_type})"
            ))
            .expect("array table");
        fixture
            .cassie
            .execute_sql(
                &fixture.session,
                "INSERT INTO array_input_copy (id, v) VALUES (0, $1)",
                vec![Value::Json(expected.clone())],
            )
            .expect("prior committed row");
        let statement = CopyStatement {
            table: "array_input_copy".into(),
            columns: vec!["id".into(), "v".into()],
            format: CopyFormat::Csv,
            header: false,
        };
        let payload = format!(
            "1,\"{}\"\n2,\"{}\"\n",
            valid.replace('"', "\"\""),
            invalid.replace('"', "\"\"")
        );

        // Act
        let result =
            fixture
                .cassie
                .copy_from_csv_stdin(&fixture.session, &statement, payload.as_bytes());

        // Assert
        assert!(
            matches!(result, Err(CassieError::Parse(_))),
            "{label}: invalid COPY input should be rejected before publication"
        );
        assert_eq!(
            fixture.rows("SELECT id, v FROM array_input_copy ORDER BY id"),
            vec![vec![Value::Int64(0), Value::Json(expected)]]
        );
    }
}
