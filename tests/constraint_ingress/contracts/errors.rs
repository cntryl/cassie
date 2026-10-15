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
