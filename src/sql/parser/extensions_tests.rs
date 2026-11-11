use crate::sql::ast::{ExtensionAction, QueryStatement};

use super::parse_extension_statement;

#[test]
fn should_parse_selected_extension_lifecycle_statements() {
    // Arrange
    let statements = [
        "CREATE EXTENSION IF NOT EXISTS pgcrypto WITH SCHEMA public VERSION '1.4'",
        "ALTER EXTENSION pgcrypto SET SCHEMA crypto_schema",
        "ALTER EXTENSION pgcrypto UPDATE TO '1.4'",
        "DROP EXTENSION IF EXISTS pgcrypto CASCADE",
    ];

    // Act
    let parsed = statements
        .iter()
        .map(|statement| parse_extension_statement(statement).expect("parse extension SQL"))
        .collect::<Vec<_>>();

    // Assert
    assert!(matches!(
        &parsed[0].statement,
        QueryStatement::Extension(crate::sql::ast::ExtensionStatement {
            action: ExtensionAction::Create {
                name,
                if_not_exists: true,
                schema: Some(schema),
                version: Some(version),
            }
        }) if name == "pgcrypto" && schema == "public" && version == "1.4"
    ));
    assert!(matches!(
        &parsed[1].statement,
        QueryStatement::Extension(crate::sql::ast::ExtensionStatement {
            action: ExtensionAction::SetSchema { name, schema },
        }) if name == "pgcrypto" && schema == "crypto_schema"
    ));
    assert!(matches!(
        &parsed[2].statement,
        QueryStatement::Extension(crate::sql::ast::ExtensionStatement {
            action: ExtensionAction::Update { name, version: Some(version) },
        }) if name == "pgcrypto" && version == "1.4"
    ));
    assert!(matches!(
        &parsed[3].statement,
        QueryStatement::Extension(crate::sql::ast::ExtensionStatement {
            action: ExtensionAction::Drop { name, if_exists: true, cascade: true },
        }) if name == "pgcrypto"
    ));
}

#[test]
fn should_reject_unselected_extension_ddl_clauses() {
    // Arrange
    let statements = [
        "CREATE EXTENSION pgcrypto FROM '1.3'",
        "ALTER EXTENSION pgcrypto ADD TABLE my_table",
        "DROP EXTENSION pgcrypto, vector",
    ];

    // Act
    for statement in statements {
        let parsed = parse_extension_statement(statement);

        // Assert
        assert!(parsed.is_err(), "{statement}");
    }
}
