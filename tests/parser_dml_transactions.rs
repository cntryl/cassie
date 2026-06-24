#![allow(unused_imports)]

use cassie::app::Cassie;
use cassie::catalog::{IndexKind, IndexMeta};
use cassie::sql::ast::{
    BinaryOp, CteQuery, Expr, InsertSource, JoinKind, QuerySource, QueryStatement, SelectItem,
    SetOperator, SortDirection,
};
use cassie::sql::parse_statement;
use cassie::types::{DataType, FieldSchema, Schema};
use std::collections::BTreeMap;
use uuid::Uuid;

#[test]
fn should_bind_insert_statement_for_existing_collection() {
    // Arrange
    let sql = "INSERT INTO docs VALUES (1)";
    let cassie = Cassie::new_with_data_dir(format!(
        "/tmp/cassie-parser-insert-binding-{}",
        Uuid::new_v4()
    ))
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie
            .catalog
            .register_collection("docs", vec![("id".to_string(), DataType::Int)]);

        // Act
        let parsed = parse_statement(sql).expect("insert statements should parse");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        // Assert
        assert!(
            bound.is_ok(),
            "insert binding should succeed for known tables"
        );
    });
}

#[test]
fn should_parse_insert_with_explicit_columns() {
    // Arrange
    let sql = "INSERT INTO docs (title) VALUES ('alpha')";

    // Act
    let parsed = parse_statement(sql).expect("insert parse");

    // Assert
    let QueryStatement::Insert(statement) = parsed.statement else {
        panic!("expected insert statement");
    };
    assert_eq!(statement.table, "docs");
    assert_eq!(statement.columns, vec!["title".to_string()]);
    let value = match &statement.source {
        InsertSource::Values(rows) => {
            let row = rows.first().expect("missing insert row");
            let value = row.first().expect("missing insert value");
            if let Expr::StringLiteral(value) = value {
                value
            } else {
                panic!("expected string literal");
            }
        }
        _ => panic!("expected values source"),
    };
    assert_eq!(value, "alpha");
}

#[test]
fn should_parse_insert_select_source() {
    // Arrange
    let sql = "INSERT INTO docs SELECT title FROM docs";

    // Act
    let parsed = parse_statement(sql).expect("insert parse");

    // Assert
    let QueryStatement::Insert(statement) = parsed.statement else {
        panic!("expected insert statement");
    };
    assert_eq!(statement.table, "docs");
    match statement.source {
        InsertSource::Select(_) => {}
        InsertSource::Values(_) => panic!("expected select source"),
    }
}

#[test]
fn should_parse_insert_on_conflict_do_nothing() {
    // Arrange
    let sql = "INSERT INTO docs VALUES (1) ON CONFLICT DO NOTHING";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Insert(statement) = parsed.statement else {
        panic!("expected insert statement");
    };
    let on_conflict = statement.on_conflict.expect("missing on conflict");
    assert!(on_conflict.target_fields.is_empty());
    assert!(matches!(
        on_conflict.action,
        cassie::sql::ast::InsertConflictAction::DoNothing
    ));
}

#[test]
fn should_parse_insert_on_conflict_do_update() {
    // Arrange
    let sql = "INSERT INTO docs (id, title) VALUES (1, 'alpha') ON CONFLICT (id) DO UPDATE SET title = excluded.title WHERE docs.id = 1";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Insert(statement) = parsed.statement else {
        panic!("expected insert statement");
    };
    let on_conflict = statement.on_conflict.expect("missing on conflict");
    assert_eq!(on_conflict.target_fields, vec!["id".to_string()]);
    let cassie::sql::ast::InsertConflictAction::DoUpdate {
        assignments,
        filter,
    } = on_conflict.action
    else {
        panic!("expected do update action");
    };
    assert_eq!(assignments.len(), 1);
    assert_eq!(assignments[0].0, "title");
    assert!(filter.is_some());
}

#[test]
fn should_parse_transaction_control_statements() {
    // Arrange
    let statements = ["BEGIN", "COMMIT", "ROLLBACK"];

    // Act
    let parsed = statements
        .iter()
        .map(|sql| parse_statement(sql))
        .collect::<Vec<_>>();

    // Assert
    assert!(parsed.iter().all(Result::is_ok));
    assert!(matches!(
        parsed[0].as_ref().unwrap().statement,
        QueryStatement::Transaction(_)
    ));
    assert!(matches!(
        parsed[1].as_ref().unwrap().statement,
        QueryStatement::Transaction(_)
    ));
    assert!(matches!(
        parsed[2].as_ref().unwrap().statement,
        QueryStatement::Transaction(_)
    ));
}

#[test]
fn should_parse_savepoint_transaction_control_statement() {
    // Arrange
    let sql = "SAVEPOINT sp";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    assert!(matches!(
        parsed.statement,
        QueryStatement::Transaction(cassie::sql::ast::TransactionStatement {
            action: cassie::sql::ast::TransactionAction::Savepoint { .. },
            ..
        })
    ));
}

#[test]
fn should_parse_quoted_savepoint_transaction_control_statement() {
    // Arrange
    let sql = r#"SAVEPOINT "_pg3_1""#;

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Transaction(statement) = parsed.statement else {
        panic!("expected transaction statement");
    };
    match statement.action {
        cassie::sql::ast::TransactionAction::Savepoint { name } => {
            assert_eq!(name, "_pg3_1");
        }
        _ => panic!("expected savepoint action"),
    }
}

#[test]
fn should_parse_rollback_to_savepoint_transaction_control_statement() {
    // Arrange
    let sql = "ROLLBACK TO SAVEPOINT sp";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    assert!(matches!(
        parsed.statement,
        QueryStatement::Transaction(cassie::sql::ast::TransactionStatement {
            action: cassie::sql::ast::TransactionAction::RollbackTo { .. },
            ..
        })
    ));
}

#[test]
fn should_parse_release_savepoint_transaction_control_statement() {
    // Arrange
    let sql = "RELEASE SAVEPOINT sp";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    assert!(matches!(
        parsed.statement,
        QueryStatement::Transaction(cassie::sql::ast::TransactionStatement {
            action: cassie::sql::ast::TransactionAction::Release { .. },
            ..
        })
    ));
}

#[test]
fn should_reject_savepoint_without_name() {
    // Arrange
    let sql = "SAVEPOINT";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_err());
    assert!(parsed.unwrap_err().0.contains("SAVEPOINT requires a name"));
}

#[test]
fn should_reject_transaction_isolation_level_changes() {
    // Arrange
    let sql = "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_err());
    assert!(parsed.unwrap_err().0.contains("unsupported"));
}

#[test]
fn should_reject_two_phase_transaction_control() {
    // Arrange
    let sql = "PREPARE TRANSACTION 'tx1'";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_err());
    assert!(parsed.unwrap_err().0.contains("unsupported"));
}
