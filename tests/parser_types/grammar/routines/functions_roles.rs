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
fn should_parse_create_function_statement() {
    // Arrange
    let sql = "CREATE FUNCTION double(x INT) RETURNS INT AS \"x * 2\"";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::CreateFunction(statement) = parsed.statement else {
        panic!("expected create function");
    };

    assert_eq!(statement.name, "double");
    assert_eq!(statement.args.len(), 1);
    assert_eq!(statement.args[0].name, "x");
    assert_eq!(statement.args[0].data_type, DataType::Int);
    assert_eq!(statement.return_type, DataType::Int);
    assert_eq!(statement.body, "x * 2");
}

#[test]
fn should_parse_create_procedure_statement() {
    // Arrange
    let sql = "CREATE PROCEDURE log_event(message TEXT) AS \"noop\"";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::CreateProcedure(statement) = parsed.statement else {
        panic!("expected create procedure");
    };

    assert_eq!(statement.name, "log_event");
    assert_eq!(statement.args.len(), 1);
    assert_eq!(statement.args[0].name, "message");
    assert_eq!(statement.args[0].data_type, DataType::Text);
    assert_eq!(statement.body, "noop");
}

#[test]
fn should_reject_unknown_function_during_binding() {
    // Arrange
    let cassie =
        Cassie::new_with_data_dir(format!("/tmp/cassie-parser-{}", Uuid::new_v4())).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie.register_collection(
            "binder_docs".to_string(),
            Schema {
                fields: vec![FieldSchema {
                    name: "id".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            },
        );

        // Act
        let parsed = parse_statement("SELECT unknown_fn(id) FROM binder_docs").unwrap();
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        // Assert
        assert!(bound.is_err());
    });
}

#[test]
fn should_reject_bad_function_arity_during_binding() {
    // Arrange
    let cassie =
        Cassie::new_with_data_dir(format!("/tmp/cassie-parser-{}", Uuid::new_v4())).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie.register_collection(
            "binder_docs_arity".to_string(),
            Schema {
                fields: vec![FieldSchema {
                    name: "id".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            },
        );

        // Act
        let parsed = parse_statement("SELECT search(id) FROM binder_docs_arity").unwrap();
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        // Assert
        assert!(bound.is_err());
    });
}

#[test]
fn should_accept_case_insensitive_function_names_during_binding() {
    // Arrange
    let cassie =
        Cassie::new_with_data_dir(format!("/tmp/cassie-parser-{}", Uuid::new_v4())).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie.register_collection(
            "binder_docs_case".to_string(),
            Schema {
                fields: vec![FieldSchema {
                    name: "body".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            },
        );

        // Act
        let parsed =
            parse_statement("SELECT SEARCH_SCORE(body, 'q') FROM binder_docs_case").unwrap();
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        // Assert
        assert!(bound.is_ok(), "function names should be case-insensitive");
    });
}

#[test]
fn should_allow_snippet_function_binding() {
    // Arrange
    let cassie =
        Cassie::new_with_data_dir(format!("/tmp/cassie-parser-{}", Uuid::new_v4())).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie.register_collection(
            "binder_docs_snippet".to_string(),
            Schema {
                fields: vec![FieldSchema {
                    name: "body".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            },
        );

        // Act
        let parsed = parse_statement("SELECT snippet(body, 'q') FROM binder_docs_snippet").unwrap();
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        // Assert
        assert!(bound.is_ok());
    });
}

#[test]
fn should_parse_create_role_statement() {
    // Arrange
    let sql = "CREATE ROLE analytics LOGIN PASSWORD 'secret'";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::CreateRole(statement) = parsed.statement else {
        panic!("expected create role");
    };

    assert_eq!(statement.name, "analytics");
    assert!(statement.login);
    assert_eq!(statement.password.as_deref(), Some("secret"));
}

#[test]
fn should_parse_alter_role_password_statement() {
    // Arrange
    let sql = "ALTER ROLE analytics PASSWORD 'rotated'";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::AlterRole(statement) = parsed.statement else {
        panic!("expected alter role");
    };

    assert_eq!(statement.name, "analytics");
    assert_eq!(statement.login, None);
    assert_eq!(statement.password.as_deref(), Some("rotated"));
}

#[test]
fn should_parse_drop_role_statement() {
    // Arrange
    let sql = "DROP ROLE analytics";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::DropRole(statement) = parsed.statement else {
        panic!("expected drop role");
    };

    assert_eq!(statement.name, "analytics");
    assert!(!statement.if_exists);
}

#[test]
fn should_reject_privilege_sql_statements() {
    // Arrange
    let statements = [
        "GRANT SELECT ON table TO public",
        "REVOKE ALL ON table FROM public",
        "CREATE POLICY tenant_policy ON docs USING (tenant_id = current_user)",
        "ALTER TABLE docs ENABLE ROW LEVEL SECURITY",
        "SET ROLE analytics",
        "SET SESSION AUTHORIZATION analytics",
    ];

    for statement in statements {
        // Act
        let result = parse_statement(statement);

        // Assert
        assert!(
            result.is_err(),
            "expected unsupported statement: {statement}"
        );
    }
}
