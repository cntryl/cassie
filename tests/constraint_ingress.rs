use cassie::types::Value;
use serde_json::json;

#[path = "support/constraint_ingress.rs"]
mod support;

use support::Fixture;

#[test]
fn should_apply_defaults_for_omitted_values_across_supported_ingress() {
    // Arrange
    let fixture = Fixture::new();

    // Act
    fixture
        .execute("INSERT INTO constraint_default_ingress (id) VALUES (1)")
        .expect("INSERT VALUES should apply default");
    fixture
        .execute(
            "INSERT INTO constraint_default_ingress (id) SELECT id FROM constraint_default_source",
        )
        .expect("INSERT SELECT should apply default");
    fixture
        .copy(&["id"], b"3\n")
        .expect("COPY target list should apply default");
    fixture
        .cassie
        .ingest_document("constraint_default_ingress", json!({"id": 4}))
        .expect("embedded ingest should apply default");
    cassie::rest::documents::create(
        &fixture.cassie,
        "constraint_default_ingress",
        br#"{"id":5}"#,
    )
    .expect("REST ingest should apply default");
    fixture
        .execute("UPDATE constraint_default_ingress SET note = 'updated' WHERE id = 1")
        .expect("partial update should preserve omitted field");

    // Assert
    assert_eq!(
        fixture.rows("SELECT id, required, note FROM constraint_default_ingress ORDER BY id"),
        vec![
            vec![
                Value::Int64(1),
                Value::String("generated".to_string()),
                Value::String("updated".to_string())
            ],
            vec![
                Value::Int64(2),
                Value::String("generated".to_string()),
                Value::String("initial".to_string())
            ],
            vec![
                Value::Int64(3),
                Value::String("generated".to_string()),
                Value::String("initial".to_string())
            ],
            vec![
                Value::Int64(4),
                Value::String("generated".to_string()),
                Value::String("initial".to_string())
            ],
            vec![
                Value::Int64(5),
                Value::String("generated".to_string()),
                Value::String("initial".to_string())
            ],
        ]
    );
}

#[test]
fn should_preserve_explicit_values_when_defaults_exist_across_ingress() {
    // Arrange
    let fixture = Fixture::new();

    // Act
    fixture
        .execute("INSERT INTO constraint_default_ingress (id, required) VALUES (1, 'values')")
        .expect("INSERT VALUES should preserve explicit value");
    fixture
        .execute("INSERT INTO constraint_default_ingress (id, required) SELECT 2, 'select'")
        .expect("INSERT SELECT should preserve explicit value");
    fixture
        .copy(&["id", "required"], b"3,copy\n")
        .expect("COPY should preserve explicit value");
    fixture
        .cassie
        .ingest_document(
            "constraint_default_ingress",
            json!({"id": 4, "required": "embedded"}),
        )
        .expect("embedded ingest should preserve explicit value");
    cassie::rest::documents::create(
        &fixture.cassie,
        "constraint_default_ingress",
        br#"{"id":5,"required":"rest"}"#,
    )
    .expect("REST ingest should preserve explicit value");

    // Assert
    assert_eq!(
        fixture.rows("SELECT id, required FROM constraint_default_ingress ORDER BY id"),
        vec![
            vec![Value::Int64(1), Value::String("values".to_string())],
            vec![Value::Int64(2), Value::String("select".to_string())],
            vec![Value::Int64(3), Value::String("copy".to_string())],
            vec![Value::Int64(4), Value::String("embedded".to_string())],
            vec![Value::Int64(5), Value::String("rest".to_string())],
        ]
    );
}

#[test]
fn should_reject_explicit_null_across_supported_ingress() {
    // Arrange
    let fixture = Fixture::new();
    fixture
        .execute("INSERT INTO constraint_default_ingress (id) VALUES (1)")
        .expect("seed defaulted row");

    // Act
    let rejected = [
        fixture.execute("INSERT INTO constraint_default_ingress (id, required) VALUES (2, NULL)"),
        fixture.execute("INSERT INTO constraint_default_ingress (id, required) SELECT 3, NULL"),
        fixture.execute("UPDATE constraint_default_ingress SET required = NULL WHERE id = 1"),
    ];
    for result in rejected {
        let error = result.expect_err("explicit SQL NULL must override the default");
        assert!(
            error.to_string().contains("cannot be null"),
            "unexpected constraint error: {error}"
        );
    }
    let copy_error = fixture
        .copy(&["id", "required"], b"4,\\N\n")
        .expect_err("COPY SQL NULL must override the default");
    assert!(copy_error.to_string().contains("cannot be null"));
    let embedded_error = fixture
        .cassie
        .ingest_document(
            "constraint_default_ingress",
            json!({"id": 5, "required": null}),
        )
        .expect_err("embedded scalar null must remain SQL NULL");
    assert!(embedded_error.to_string().contains("cannot be null"));
    let rest_error = cassie::rest::documents::create(
        &fixture.cassie,
        "constraint_default_ingress",
        br#"{"id":6,"required":null}"#,
    )
    .expect_err("REST scalar null must remain SQL NULL");
    assert!(rest_error.to_string().contains("cannot be null"));

    // Assert
    assert_eq!(
        fixture.rows("SELECT id, required FROM constraint_default_ingress ORDER BY id"),
        vec![vec![
            Value::Int64(1),
            Value::String("generated".to_string())
        ]],
        "rejected writes must not publish rows or mutate the existing row"
    );
}
