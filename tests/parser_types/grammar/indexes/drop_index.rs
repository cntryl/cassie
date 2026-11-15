#[test]
fn should_parse_drop_index_statement() {
    // Arrange
    let sql = "DROP INDEX IF EXISTS idx_users_email ON users";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::DropIndex(statement) = parsed.statement else {
        panic!("expected drop index statement");
    };

    assert_eq!(statement.name, "idx_users_email");
    assert_eq!(statement.table, "users");
    assert!(statement.if_exists);
}

#[test]
fn should_parse_drop_index_without_an_on_clause() {
    // Arrange
    let sql = "DROP INDEX IF EXISTS idx_users_email";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::DropIndex(statement) = parsed.statement else {
        panic!("expected drop index statement");
    };

    assert_eq!(statement.name, "idx_users_email");
    assert_eq!(statement.table, "");
    assert!(statement.if_exists);
}

#[test]
fn should_parse_quoted_drop_index_name_without_an_on_clause() {
    // Arrange
    let sql = r#"DROP INDEX "index with spaces""#;

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::DropIndex(statement) = parsed.statement else {
        panic!("expected drop index statement");
    };

    assert_eq!(statement.name, r#""index with spaces""#);
    assert_eq!(statement.table, "");
}

#[test]
fn should_reject_drop_index_concurrently_as_unsupported_syntax() {
    // Arrange
    let sql = "DROP INDEX CONCURRENTLY customer_orders_customer_id_idx";

    // Act
    let error = parse_statement(sql).expect_err("concurrent index maintenance is unsupported");

    // Assert
    assert!(error
        .to_string()
        .contains("DROP INDEX CONCURRENTLY is not supported"));
}
