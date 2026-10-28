#![allow(unused_imports)]

use cassie::app::Cassie;
use cassie::catalog::{IndexKind, IndexMeta};
use cassie::sql::ast::{
    BinaryOp, CopyFormat, CteQuery, Expr, InsertSource, JoinKind, QuerySource, QueryStatement,
    SelectItem, SetOperator, SortDirection,
};
use cassie::sql::parse_statement;
use cassie::sql::IdentifierPath;
use cassie::types::{DataType, FieldSchema, Schema};
use std::collections::BTreeMap;
use uuid::Uuid;

#[test]
fn should_parse_select_statement_with_aliases_filters_sorting_pagination() {
    // Arrange
    let sql = "SELECT title AS doc_title, search_score(body, 'world') AS score FROM docs WHERE active = true AND title <> 'bad' ORDER BY score DESC, id LIMIT 10 OFFSET 5";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Select(statement) = parsed.statement else {
        panic!("expected select statement");
    };

    assert_eq!(
        statement.source,
        QuerySource::Collection(IdentifierPath::parse("docs").expect("relation path"))
    );
    assert!(matches!(statement.limit, Some(Expr::IntegerLiteral(10))));
    assert!(matches!(statement.offset, Some(Expr::IntegerLiteral(5))));

    assert_eq!(statement.projection.len(), 2);
    match &statement.projection[0] {
        SelectItem::Column { name, alias } => {
            assert_eq!(name, "title");
            assert_eq!(alias.as_deref(), Some("doc_title"));
        }
        _ => panic!("expected column projection"),
    }

    match &statement.projection[1] {
        SelectItem::Function { function, alias } => {
            assert_eq!(function.name, "search_score");
            assert_eq!(alias.as_deref(), Some("score"));
        }
        _ => panic!("expected function projection"),
    }

    let filter = statement.filter.expect("filter expected");
    let Expr::Binary { .. } = filter else {
        panic!("filter should be binary")
    };

    assert_eq!(statement.order.len(), 2);
    assert!(matches!(statement.order[0].direction, SortDirection::Desc));
    assert!(matches!(statement.order[1].direction, SortDirection::Asc));
}

#[test]
fn should_parse_show_statement_with_variable() {
    // Arrange
    let sql = "SHOW search_path";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Show(statement) = parsed.statement else {
        panic!("expected show statement");
    };

    assert_eq!(statement.variable, "search_path");
}

#[test]
fn should_parse_set_statement_with_equals_form() {
    // Arrange
    let sql = "SET search_path = public";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Set(statement) = parsed.statement else {
        panic!("expected set statement");
    };

    assert_eq!(statement.variable, "search_path");
    assert_eq!(statement.value.as_deref(), Some("public"));
}

#[test]
fn should_parse_set_statement_with_to_form() {
    // Arrange
    let sql = "SET search_path TO public";

    // Act
    let parsed = parse_statement(sql).expect("parse should succeed");

    // Assert
    let QueryStatement::Set(statement) = parsed.statement else {
        panic!("expected set statement");
    };

    assert_eq!(statement.variable, "search_path");
    assert_eq!(statement.value.as_deref(), Some("public"));
}

#[test]
fn should_target_client_encoding_given_set_names_when_encoding_literals_are_parsed() {
    // Arrange
    let cases = [
        ("SET NAMES 'UTF8'", "UTF8"),
        ("SET NAMES 'UTF-8'", "UTF-8"),
        ("SET SESSION NAMES 'UTF8'", "UTF8"),
        ("sEt\tSeSsIoN\nNaMeS 'UTF8'", "UTF8"),
        (
            "SET/*scope*/SESSION/*alias*/NAMES/*encoding*/'UTF8'",
            "UTF8",
        ),
        ("SET NAMES $$UTF8$$", "UTF8"),
        ("SET NAMES 'LATIN1'", "LATIN1"),
        ("SET NAMES 'unknown''encoding'", "unknown'encoding"),
    ];

    // Act
    let parsed = cases
        .iter()
        .map(|(sql, _)| parse_statement(sql).expect("SET NAMES string literal parses"))
        .collect::<Vec<_>>();

    // Assert
    for (parsed, (sql, encoding)) in parsed.into_iter().zip(cases) {
        let QueryStatement::Set(statement) = parsed.statement else {
            panic!("expected SET statement for {sql}");
        };
        assert_eq!(statement.variable, "client_encoding", "{sql}");
        assert_eq!(statement.value.as_deref(), Some(encoding), "{sql}");
    }
}

#[test]
fn should_preserve_default_setting_representation_given_set_names_when_encoding_is_omitted() {
    // Arrange
    let sql = [
        "SET NAMES",
        "SET NAMES DEFAULT",
        "SET SESSION NAMES",
        "SET SESSION NAMES DEFAULT",
        "SET NAMES /*use reset default*/",
    ];
    let QueryStatement::Set(default) = parse_statement("SET client_encoding TO DEFAULT")
        .expect("existing DEFAULT syntax")
        .statement
    else {
        panic!("expected SET statement");
    };

    // Act
    let parsed = sql
        .iter()
        .map(|sql| parse_statement(sql).expect("SET NAMES default parses"))
        .collect::<Vec<_>>();

    // Assert
    for (parsed, sql) in parsed.into_iter().zip(sql) {
        let QueryStatement::Set(statement) = parsed.statement else {
            panic!("expected SET statement for {sql}");
        };
        assert_eq!(statement.variable, default.variable, "{sql}");
        assert_eq!(statement.value, default.value, "{sql}");
        assert_eq!(statement.action, default.action, "{sql}");
    }
}

#[test]
fn should_preserve_generic_names_setting_given_assignment_syntax_when_set_is_parsed() {
    // Arrange
    let cases = ["SET NAMES = 'UTF8'", "SET NAMES TO 'UTF8'"];

    // Act
    let parsed = cases
        .iter()
        .map(|sql| parse_statement(sql).expect("generic configuration assignment parses"))
        .collect::<Vec<_>>();

    // Assert
    for (parsed, sql) in parsed.into_iter().zip(cases) {
        let QueryStatement::Set(statement) = parsed.statement else {
            panic!("expected SET statement for {sql}");
        };
        assert!(statement.variable.eq_ignore_ascii_case("names"), "{sql}");
        assert_eq!(statement.value.as_deref(), Some("UTF8"), "{sql}");
    }
}

#[test]
fn should_reject_invalid_encoding_syntax_given_set_names_when_nonliteral_tokens_are_present() {
    // Arrange
    let cases = [
        "SET NAMES UTF8",
        "SET NAMES \"UTF8\"",
        "SET NAMES ('UTF8')",
        "SET NAMES 'UTF8' ignored",
        "SET NAMES 'UTF8', 'LATIN1'",
        "SET NAMES DEFAULT ignored",
        "SET NAMES 'UTF8' COLLATE 'C'",
        "SET SESSION NAMES 'UTF8' ignored",
    ];

    // Act
    let errors = cases
        .iter()
        .map(|sql| parse_statement(sql).expect_err("SET NAMES accepts one string or DEFAULT"))
        .collect::<Vec<_>>();

    // Assert
    for (error, sql) in errors.into_iter().zip(cases) {
        assert_eq!(error.kind(), cassie::sql::SqlErrorKind::Syntax, "{sql}");
    }
}

#[test]
fn should_parse_non_select_statement() {
    // Arrange
    let sql = "INSERT INTO docs VALUES (1)";

    // Act
    let parsed = parse_statement(sql).expect("insert statements should parse");

    // Assert
    assert!(matches!(parsed.statement, QueryStatement::Insert(_)));
}

#[test]
fn should_parse_copy_from_stdin_csv_column_header_shape() {
    // Arrange
    let sql = "COPY docs (_id, title, score) FROM STDIN WITH (FORMAT csv, HEADER true)";

    // Act
    let parsed = parse_statement(sql).expect("copy statements should parse");

    // Assert
    let QueryStatement::Copy(statement) = parsed.statement else {
        panic!("expected copy statement");
    };
    assert_eq!(statement.table, "docs");
    assert_eq!(statement.columns, vec!["_id", "title", "score"]);
    assert_eq!(statement.format, CopyFormat::Csv);
    assert!(statement.header);
}

#[test]
fn should_reject_copy_from_stdin_non_csv_format() {
    // Arrange
    let sql = "COPY docs FROM STDIN WITH (FORMAT binary)";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_err());
}

#[test]
fn should_parse_deterministically_across_invocations() {
    // Arrange
    let sql_one =
        "SELECT title AS doc_title, search_score(body, 'world') AS score FROM docs WHERE active = true ORDER BY score DESC LIMIT 1 OFFSET 0";
    let sql_two =
        "select title AS doc_title, search_score(body, 'world') AS score from docs where active = true order by score desc limit 1 offset 0";

    // Act
    let first = parse_statement(sql_one).unwrap();
    let second = parse_statement(sql_two).unwrap();

    // Assert
    let ((), first_statement) = match first.statement {
        QueryStatement::Select(statement) => ((), statement),
        _ => panic!("expected select statement"),
    };
    let ((), second_statement) = match second.statement {
        QueryStatement::Select(statement) => ((), statement),
        _ => panic!("expected select statement"),
    };

    assert_eq!(
        format!("{first_statement:?}"),
        format!("{:?}", second_statement)
    );
}

#[test]
fn should_parse_named_window_clause_in_query() {
    // Arrange
    let sql = "SELECT * FROM docs WINDOW ranked AS (PARTITION BY title)";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_ok());
}

#[test]
fn should_reject_duplicate_limit_clauses() {
    // Arrange
    let sql = "SELECT * FROM docs LIMIT 1 LIMIT 2";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_err());
}

#[test]
fn should_parse_signed_limit_literals_before_execution() {
    // Arrange
    let negative_limit = parse_statement("SELECT * FROM docs LIMIT -1");
    let zero_limit = parse_statement("SELECT * FROM docs LIMIT 0");

    // Act
    let zero_offset = parse_statement("SELECT * FROM docs OFFSET 0");

    // Assert
    assert!(negative_limit.is_ok());
    assert!(zero_limit.is_ok());
    assert!(zero_offset.is_ok());
}

#[test]
fn should_accept_zero_offset_values() {
    // Arrange
    let zero_offset = parse_statement("SELECT * FROM docs OFFSET 0");

    // Act
    let parsed = zero_offset;

    // Assert
    assert!(parsed.is_ok());
}

#[test]
fn should_reject_malformed_parameter_tokens() {
    // Arrange
    let missing_number = parse_statement("SELECT * FROM docs WHERE title = $");
    let non_numeric = parse_statement("SELECT * FROM docs WHERE title = $x");

    // Act
    let zero_index = parse_statement("SELECT * FROM docs WHERE title = $0");

    // Assert
    assert!(missing_number.is_err());
    assert!(non_numeric.is_err());
    assert!(zero_index.is_err());
}

#[test]
fn should_reject_unknown_trailing_tokens_after_query() {
    // Arrange
    let sql = "SELECT * FROM docs WHERE title = 'a' FOO";

    // Act
    let parsed = parse_statement(sql);

    // Assert
    assert!(parsed.is_err());
}

#[test]
fn should_reject_unresolvable_order_by_identifier_during_binding() {
    // Arrange
    let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie
            .register_collection(
                "binder_docs_order_alias".to_string(),
                Schema {
                    fields: vec![FieldSchema {
                        name: "body".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    }],
                },
            );

        // Act
        let parsed = parse_statement(
            "SELECT search_score(body, 'world') AS score FROM binder_docs_order_alias ORDER BY missing_alias",
        )
        .unwrap();
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        // Assert
        assert!(bound.is_err());
    });
}

#[test]
fn should_allow_projection_alias_order_by_during_binding() {
    // Arrange
    let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie
            .register_collection(
                "binder_docs_order_alias_ok".to_string(),
                Schema {
                    fields: vec![FieldSchema {
                        name: "body".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    }],
                },
            );

        // Act
        let parsed = parse_statement(
            "SELECT search_score(body, 'world') AS Score FROM binder_docs_order_alias_ok ORDER BY score",
        )
        .unwrap();
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        // Assert
        assert!(bound.is_ok());
    });
}

#[test]
fn should_reject_unknown_projection_column_during_binding() {
    // Arrange
    let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        cassie.register_collection(
            "binder_docs_projection_col".to_string(),
            Schema {
                fields: vec![FieldSchema {
                    name: "body".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            },
        );

        // Act
        let parsed = parse_statement("SELECT unknown FROM binder_docs_projection_col").unwrap();
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        // Assert
        assert!(bound.is_err());
    });
}

#[test]
fn should_distinguish_reset_operations_given_settings_sql_when_default_is_quoted_or_unquoted() {
    // Arrange
    use cassie::sql::ast::SettingAction;
    let cases = [
        (
            "SET SESSION application_name TO DEFAULT",
            SettingAction::SetDefault,
            "application_name",
            None,
        ),
        (
            "SET application_name TO 'DEFAULT'",
            SettingAction::Set,
            "application_name",
            Some("DEFAULT"),
        ),
        (
            "RESET application_name",
            SettingAction::Reset,
            "application_name",
            None,
        ),
        ("RESET ALL", SettingAction::ResetAll, "all", None),
    ];

    // Act
    let statements = cases
        .each_ref()
        .map(|(sql, _, _, _)| parse_statement(sql).expect("settings SQL"));

    // Assert
    for (parsed, (_, action, variable, value)) in statements.into_iter().zip(cases) {
        let QueryStatement::Set(statement) = parsed.statement else {
            panic!("settings statement expected");
        };
        assert_eq!(statement.action, action);
        assert_eq!(statement.variable, variable);
        assert_eq!(statement.value.as_deref(), value);
    }
}
