#![allow(unused_imports)]

use cassie::app::Cassie;
use cassie::catalog::{IndexKind, IndexMeta};
use cassie::sql::ast::{
    BinaryOp, CteQuery, Expr, InsertSource, JoinKind, QuerySource, QueryStatement, SelectItem,
    SetOperator, SortDirection, TransactionAction, TransactionIsolation,
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
        InsertSource::Select(_) => panic!("expected values source"),
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
    assert_eq!(on_conflict.target_fields, [] as [std::string::String; 0]);
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
    let statements = [
        ("BEGIN", TransactionAction::Begin),
        ("COMMIT", TransactionAction::Commit),
        ("ROLLBACK", TransactionAction::Rollback),
    ];

    // Act
    let parsed = statements
        .iter()
        .map(|(sql, _)| parse_statement(sql).expect("transaction control parses"))
        .collect::<Vec<_>>();

    // Assert
    for (parsed, (sql, expected)) in parsed.into_iter().zip(statements) {
        let QueryStatement::Transaction(statement) = parsed.statement else {
            panic!("expected transaction statement for {sql}");
        };
        assert_eq!(statement.action, expected, "{sql}");
        assert_eq!(statement.isolation, None, "{sql}");
    }
}

#[test]
fn should_preserve_begin_isolation_given_noise_words_when_transaction_aliases_are_parsed() {
    // Arrange
    let statements = [
        ("BEGIN WORK", None),
        ("BEGIN TRANSACTION", None),
        ("START TRANSACTION", None),
        (
            "BEGIN WORK ISOLATION LEVEL READ COMMITTED",
            Some(TransactionIsolation::ReadCommitted),
        ),
        (
            "BEGIN TRANSACTION ISOLATION LEVEL READ COMMITTED",
            Some(TransactionIsolation::ReadCommitted),
        ),
    ];

    // Act
    let parsed = statements
        .iter()
        .map(|(sql, _)| parse_statement(sql).expect("begin alias parses"))
        .collect::<Vec<_>>();

    // Assert
    for (parsed, (sql, isolation)) in parsed.into_iter().zip(statements) {
        let QueryStatement::Transaction(statement) = parsed.statement else {
            panic!("expected transaction statement for {sql}");
        };
        assert_eq!(statement.action, TransactionAction::Begin, "{sql}");
        assert_eq!(statement.isolation, isolation, "{sql}");
    }
}

#[test]
fn should_preserve_action_given_end_aliases_and_no_chain_when_transactions_are_parsed() {
    // Arrange
    let mut statements = Vec::new();
    for (command, action) in [
        ("COMMIT", TransactionAction::Commit),
        ("END", TransactionAction::Commit),
        ("ROLLBACK", TransactionAction::Rollback),
        ("ABORT", TransactionAction::Rollback),
    ] {
        for noise_word in ["", " WORK", " TRANSACTION"] {
            for suffix in ["", " AND NO CHAIN"] {
                statements.push((format!("{command}{noise_word}{suffix}"), action.clone()));
            }
        }
    }

    // Act
    let parsed = statements
        .iter()
        .map(|(sql, _)| parse_statement(sql).expect("transaction end alias parses"))
        .collect::<Vec<_>>();

    // Assert
    for (parsed, (sql, expected)) in parsed.into_iter().zip(statements) {
        let QueryStatement::Transaction(statement) = parsed.statement else {
            panic!("expected transaction statement for {sql}");
        };
        assert_eq!(statement.action, expected, "{sql}");
        assert_eq!(statement.isolation, None, "{sql}");
    }
}

#[test]
fn should_preserve_savepoint_given_rollback_noise_words_when_transaction_aliases_are_parsed() {
    // Arrange
    let statements = [
        "ROLLBACK WORK TO keep_first",
        "ROLLBACK WORK TO SAVEPOINT keep_first",
        "ROLLBACK TRANSACTION TO keep_first",
        "ROLLBACK TRANSACTION TO SAVEPOINT keep_first",
    ];

    // Act
    let parsed = statements
        .iter()
        .map(|sql| parse_statement(sql).expect("rollback alias parses"))
        .collect::<Vec<_>>();

    // Assert
    for (parsed, sql) in parsed.into_iter().zip(statements) {
        let QueryStatement::Transaction(statement) = parsed.statement else {
            panic!("expected transaction statement for {sql}");
        };
        assert_eq!(
            statement.action,
            TransactionAction::RollbackTo {
                name: "keep_first".to_string()
            },
            "{sql}"
        );
        assert_eq!(statement.isolation, None, "{sql}");
    }
}

#[test]
fn should_reject_invalid_suffixes_given_transaction_aliases_when_controls_are_parsed() {
    // Arrange
    let statements = [
        "BEGIN WORK ignored",
        "COMMIT WORK AND NO CHAIN ignored",
        "END TRANSACTION ignored",
        "ROLLBACK TRANSACTION AND NO CHAIN ignored",
        "ABORT WORK ignored",
        "ABORT TO SAVEPOINT keep_first",
        "ROLLBACK WORK TO",
        "ROLLBACK TRANSACTION TO",
    ];

    // Act
    let errors = statements
        .iter()
        .map(|sql| parse_statement(sql).expect_err("transaction aliases must consume every token"))
        .collect::<Vec<_>>();

    // Assert
    for (error, sql) in errors.into_iter().zip(statements) {
        let expected_message = if sql.ends_with(" TO") {
            "ROLLBACK TO requires a savepoint name"
        } else {
            "unsupported transaction control statement"
        };
        assert_eq!(error.message(), expected_message, "{sql}");
    }
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
    let error = parse_statement(sql).expect_err("SAVEPOINT without a name should fail");

    // Assert
    assert_eq!(error.kind(), cassie::sql::SqlErrorKind::Syntax);
    assert!(error.message().contains("SAVEPOINT requires a name"));
}

#[test]
fn should_reject_transaction_isolation_level_changes() {
    // Arrange
    let sql = "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE";

    // Act
    let error = parse_statement(sql).expect_err("unsupported isolation changes should fail");

    // Assert
    assert_eq!(error.kind(), cassie::sql::SqlErrorKind::Unsupported);
    assert!(error.message().contains("unsupported"));
}

#[test]
fn should_reject_two_phase_transaction_control() {
    // Arrange
    let sql = "PREPARE TRANSACTION 'tx1'";

    // Act
    let error = parse_statement(sql).expect_err("two-phase transaction control should fail");

    // Assert
    assert_eq!(error.kind(), cassie::sql::SqlErrorKind::Unsupported);
    assert!(error.message().contains("unsupported"));
}

#[test]
fn should_preserve_characteristics_given_multiple_begin_modes_when_transaction_sql_is_parsed() {
    // Arrange
    let cases = [
        "BEGIN WORK ISOLATION LEVEL READ UNCOMMITTED, READ ONLY, DEFERRABLE",
        "START TRANSACTION READ ONLY DEFERRABLE ISOLATION LEVEL READ UNCOMMITTED",
    ];

    // Act
    let statements = cases.map(|sql| parse_statement(sql).expect("valid transaction modes"));

    // Assert
    for parsed in statements {
        let QueryStatement::Transaction(statement) = parsed.statement else {
            panic!("transaction expected");
        };
        assert_eq!(statement.action, TransactionAction::Begin);
        assert_eq!(
            statement.isolation,
            Some(TransactionIsolation::ReadUncommitted)
        );
        assert_eq!(statement.modes.read_only, Some(true));
        assert_eq!(statement.modes.deferrable, Some(true));
        assert!(!statement.chain);
    }
}

#[test]
fn should_distinguish_chain_given_end_controls_when_transaction_sql_is_parsed() {
    // Arrange
    let cases = [
        ("COMMIT AND CHAIN", TransactionAction::Commit, true),
        ("END TRANSACTION AND CHAIN", TransactionAction::Commit, true),
        ("ROLLBACK WORK AND CHAIN", TransactionAction::Rollback, true),
        ("ABORT AND NO CHAIN", TransactionAction::Rollback, false),
    ];

    // Act
    let statements = cases
        .each_ref()
        .map(|(sql, _, _)| parse_statement(sql).expect("transaction chain control"));

    // Assert
    for (parsed, (_, action, chain)) in statements.into_iter().zip(cases) {
        let QueryStatement::Transaction(statement) = parsed.statement else {
            panic!("transaction expected");
        };
        assert_eq!(statement.action, action);
        assert_eq!(statement.chain, chain);
    }
}

#[test]
fn should_reject_incomplete_modes_given_begin_options_when_transaction_sql_is_parsed() {
    // Arrange
    let cases = [
        "BEGIN READ",
        "BEGIN READ ONLY,",
        "BEGIN , READ WRITE",
        "BEGIN ISOLATION LEVEL READ",
        "BEGIN NOT",
        "COMMIT AND CHAIN ignored",
    ];

    // Act
    let errors = cases.map(|sql| parse_statement(sql).expect_err("incomplete mode"));

    // Assert
    for error in errors {
        assert!(matches!(
            error.kind(),
            cassie::sql::parser::SqlErrorKind::Syntax
        ));
    }
}
