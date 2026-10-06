//! This fixture calls embedded `execute_sql`; the private wire profile is absent.

use super::support_sql::{data_dir, use_local_storage};
use cassie::app::Cassie;
use cassie::types::Value;

#[test]
fn should_preserve_projected_wildcard_identity_at_the_core_result_boundary() {
    // Arrange
    use_local_storage();
    let path = data_dir("type-output-core-wildcard-boundary");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE core_wildcard_plain (label TEXT, n INT)",
        "INSERT INTO core_wildcard_plain (label, n) VALUES ('alpha', 7)",
        "CREATE TABLE core_wildcard_real_id (id INT, label TEXT)",
        "INSERT INTO core_wildcard_real_id (id, label) VALUES (37, 'stored')",
    ] {
        cassie.execute_sql(&session, sql, Vec::new()).expect(sql);
    }
    let cases = [
        (
            "SELECT * FROM core_wildcard_plain",
            vec!["id", "label", "n"],
        ),
        (
            "WITH c AS (SELECT * FROM core_wildcard_plain) SELECT * FROM c",
            vec!["id", "label", "n"],
        ),
        (
            "SELECT * FROM (SELECT * FROM core_wildcard_plain) AS d",
            vec!["id", "label", "n"],
        ),
        (
            "WITH c AS (SELECT label, n FROM core_wildcard_plain) SELECT * FROM c",
            vec!["label", "n"],
        ),
        (
            "SELECT * FROM (SELECT label, n FROM core_wildcard_plain) AS d",
            vec!["label", "n"],
        ),
        ("SELECT * FROM core_wildcard_real_id", vec!["id", "label"]),
        ("SELECT * FROM core_wildcard_plain CROSS JOIN core_wildcard_real_id", vec!["label", "n", "id", "label"]),
        ("WITH c AS (SELECT * FROM core_wildcard_plain) SELECT * FROM c CROSS JOIN core_wildcard_real_id", vec!["label", "n", "id", "label"]),
        ("SELECT * FROM (SELECT * FROM core_wildcard_plain) AS d CROSS JOIN core_wildcard_real_id", vec!["label", "n", "id", "label"]),
    ];

    // Act
    let outputs = cases
        .iter()
        .map(|(sql, _)| cassie.execute_sql(&session, sql, Vec::new()).expect(sql))
        .collect::<Vec<_>>();

    // Assert
    let identity = outputs[0].rows[0][0].clone();
    assert!(matches!(&identity, Value::String(value) if !value.is_empty()));
    for ((sql, names), output) in cases.into_iter().zip(outputs) {
        assert_eq!(
            output
                .columns
                .iter()
                .map(|column| column.name.as_str())
                .collect::<Vec<_>>(),
            names,
            "{sql}"
        );
        assert_eq!(output.rows.len(), 1, "{sql}");
        assert_eq!(output.rows[0].len(), output.columns.len(), "{sql}");
        let expected = match names.as_slice() {
            ["id", "label", "n"] => vec![
                identity.clone(),
                Value::String("alpha".to_string()),
                Value::Int64(7),
            ],
            ["label", "n"] => vec![Value::String("alpha".to_string()), Value::Int64(7)],
            ["id", "label"] => vec![Value::Int64(37), Value::String("stored".to_string())],
            ["label", "n", "id", "label"] => vec![
                Value::String("alpha".to_string()),
                Value::Int64(7),
                Value::Int64(37),
                Value::String("stored".to_string()),
            ],
            _ => panic!("unknown fixed core counterprobe shape"),
        };
        assert_eq!(output.rows, vec![expected], "{sql}");
    }
    let _ = std::fs::remove_dir_all(path);
}
