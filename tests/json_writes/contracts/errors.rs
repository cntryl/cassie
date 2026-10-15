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
