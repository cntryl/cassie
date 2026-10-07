// Consolidated integration suite: parser_types.
// Shared fixtures live in tests/support; former test targets remain named modules.

#[path = "parser_types/dialect_syntax.rs"]
mod dialect_syntax;

#[path = "support/pgwire.rs"]
mod support_pgwire;
#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/sql_fixture.rs"]
mod support_sql_fixture;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;

// Formerly tests/parser_core.rs.
mod parser_core {
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
        assert_eq!(statement.limit, Some(10));
        assert_eq!(statement.offset, Some(5));

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
    fn should_reject_unknown_clause_in_query() {
        // Arrange
        let sql = "SELECT * FROM docs WINDOW ranked AS (PARTITION BY title)";

        // Act
        let parsed = parse_statement(sql);

        // Assert
        assert!(parsed.is_err());
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
    fn should_reject_invalid_limit_values() {
        // Arrange
        let negative_limit = parse_statement("SELECT * FROM docs LIMIT -1");
        let zero_limit = parse_statement("SELECT * FROM docs LIMIT 0");

        // Act
        let zero_offset = parse_statement("SELECT * FROM docs OFFSET 0");

        // Assert
        assert!(negative_limit.is_err());
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
        let cassie =
            Cassie::new_with_data_dir(format!("/tmp/cassie-parser-{}", Uuid::new_v4())).unwrap();
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
        let cassie =
            Cassie::new_with_data_dir(format!("/tmp/cassie-parser-{}", Uuid::new_v4())).unwrap();
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
        let cassie =
            Cassie::new_with_data_dir(format!("/tmp/cassie-parser-{}", Uuid::new_v4())).unwrap();
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
}

// Non-ASCII SQL text must keep byte offsets on UTF-8 character boundaries.
mod sql_text_utf8_offsets {
    use super::support_sql as support;

    use cassie::app::{Cassie, CassieSession};
    use cassie::executor::QueryResult;
    use cassie::types::Value;

    use support::{data_dir, use_local_storage};

    fn with_session(label: &str, test: impl FnOnce(&Cassie, &CassieSession)) {
        use_local_storage();
        let path = data_dir(label);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
            cassie.startup().expect("start Cassie");
            let session = cassie.create_session("tester", None);
            test(&cassie, &session);
        });
        let _ = std::fs::remove_dir_all(path);
    }

    fn run(cassie: &Cassie, session: &CassieSession, sql: &str) -> QueryResult {
        match cassie.execute_sql(session, sql, vec![]) {
            Ok(result) => result,
            Err(error) => panic!("statement failed: {sql}: {error}"),
        }
    }

    #[test]
    fn should_filter_on_non_ascii_string_literals_without_panicking() {
        with_session("utf8_literal_filter", |cassie, session| {
            // Arrange
            let names = ["café", "Müller", "naïve", "日本語", "straße", "😀"];
            run(cassie, session, "CREATE TABLE utf8_names (name TEXT)");
            for name in names {
                run(
                    cassie,
                    session,
                    &format!("INSERT INTO utf8_names (name) VALUES ('{name}')"),
                );
            }

            // Act
            let matched = names.map(|name| {
                run(
                    cassie,
                    session,
                    &format!("SELECT name FROM utf8_names WHERE name = '{name}'"),
                )
                .rows
            });

            // Assert
            for (rows, name) in matched.iter().zip(names) {
                assert_eq!(rows, &vec![vec![Value::String(name.to_string())]]);
            }
        });
    }

    #[test]
    fn should_keep_clause_boundaries_when_lowercasing_changes_byte_length() {
        with_session("utf8_lowercase_offsets", |cassie, session| {
            // Arrange
            run(cassie, session, "CREATE TABLE utf8_offsets (a TEXT)");
            run(
                cassie,
                session,
                "INSERT INTO utf8_offsets (a) VALUES ('\u{212A}é')",
            );

            // Act
            let aliased = run(cassie, session, "SELECT 'İé' AS x FROM utf8_offsets");
            let limited = run(
                cassie,
                session,
                "SELECT a FROM utf8_offsets WHERE a = '\u{212A}é' LIMIT 1",
            );

            // Assert
            assert_eq!(aliased.columns[0].name, "x");
            assert_eq!(
                limited.rows,
                vec![vec![Value::String("\u{212A}é".to_string())]]
            );
        });
    }

    #[test]
    fn should_rename_columns_whose_lowercase_changes_byte_length() {
        with_session("utf8_rename_column", |cassie, session| {
            // Arrange
            run(cassie, session, "CREATE TABLE utf8_rename (\"İa\" TEXT)");
            run(
                cassie,
                session,
                "INSERT INTO utf8_rename (\"İa\") VALUES ('v')",
            );

            // Act
            run(
                cassie,
                session,
                "ALTER TABLE utf8_rename RENAME COLUMN \"İa\" TO b",
            );
            let selected = run(cassie, session, "SELECT b FROM utf8_rename");

            // Assert
            assert_eq!(selected.rows, vec![vec![Value::String("v".to_string())]]);
        });
    }

    #[test]
    fn should_create_tables_whose_names_contain_multibyte_characters() {
        with_session("utf8_table_names", |cassie, session| {
            // Arrange
            let statements = [
                "CREATE TABLE ét (id BIGINT)",
                "CREATE TABLE \"éh\" (id BIGINT, note VARCHAR(20))",
            ];

            // Act
            let created = statements.map(|sql| run(cassie, session, sql).command);
            run(
                cassie,
                session,
                "INSERT INTO \"éh\" (id, note) VALUES (1, 'n')",
            );
            let selected = run(cassie, session, "SELECT note FROM \"éh\"");

            // Assert
            assert_eq!(created, ["CREATE TABLE", "CREATE TABLE"]);
            assert_eq!(selected.rows, vec![vec![Value::String("n".to_string())]]);
        });
    }
}

// Formerly tests/parser_cte_schema.rs.
mod parser_cte_schema {
    #![allow(unused_imports)]

    use cassie::app::Cassie;
    use cassie::catalog::{CollectionStorageMode, IndexKind, IndexMeta};
    use cassie::sql::ast::{
        BinaryOp, CteQuery, Expr, InsertSource, JoinKind, QuerySource, QueryStatement, SelectItem,
        SetOperator, SortDirection,
    };
    use cassie::sql::parse_statement;
    use cassie::sql::IdentifierPath;
    use cassie::types::{DataType, FieldSchema, Schema};
    use std::collections::BTreeMap;
    use uuid::Uuid;

    #[test]
    fn should_parse_with_clause_with_cte_source() {
        // Arrange
        let sql = "WITH docs_cte AS (SELECT title FROM docs) SELECT title FROM docs_cte";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        assert_eq!(statement.ctes.len(), 1);
        assert_eq!(statement.ctes[0].name, "docs_cte");
        assert!(matches!(statement.ctes[0].query, CteQuery::Simple(_)));
        assert_eq!(
            statement.source,
            QuerySource::Collection(IdentifierPath::parse("docs_cte").expect("relation path"),)
        );
    }

    #[test]
    fn should_parse_multiple_ctes_with_dependencies() {
        // Arrange
        let sql = "WITH first AS (SELECT title FROM docs), second AS (SELECT title FROM first) SELECT title FROM second";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        assert_eq!(statement.ctes.len(), 2);
        assert_eq!(statement.ctes[0].name, "first");
        assert_eq!(statement.ctes[1].name, "second");
        assert_eq!(
            statement.source,
            QuerySource::Collection(IdentifierPath::parse("second").expect("relation path"))
        );
    }

    #[test]
    fn should_parse_recursive_cte_shape() {
        // Arrange
        let sql = "with recursive counter(n) as (SELECT n FROM docs UNION ALL SELECT n FROM counter) SELECT n FROM counter";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        assert!(statement.recursive);
        assert_eq!(statement.ctes.len(), 1);
        assert_eq!(statement.ctes[0].aliases, vec!["n".to_string()]);
        assert!(matches!(
            statement.ctes[0].query,
            CteQuery::Recursive { .. }
        ));
    }

    #[test]
    fn should_parse_cte_column_aliases() {
        // Arrange
        let sql =
        "WITH docs_cte(title_alias) AS (SELECT title FROM docs) SELECT title_alias FROM docs_cte";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        assert_eq!(statement.ctes[0].aliases.len(), 1);
        assert_eq!(statement.ctes[0].aliases[0], "title_alias");
        assert_eq!(
            statement.source,
            QuerySource::Collection(IdentifierPath::parse("docs_cte").expect("relation path"),)
        );
    }

    #[test]
    fn should_parse_create_table_with_if_not_exists() {
        // Arrange
        let sql =
        "CREATE TABLE IF NOT EXISTS users (id INT, title TEXT, embedding VECTOR(3), flag BOOLEAN)";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::CreateTable(statement) = parsed.statement else {
            panic!("expected create table statement");
        };

        assert_eq!(statement.table, "users");
        assert!(statement.if_not_exists);
        assert_eq!(statement.fields.len(), 4);
        assert_eq!(statement.fields[1].name, "title");
        assert_eq!(statement.fields[1].data_type, DataType::Text);
    }

    #[test]
    fn should_parse_create_graph_field_sections() {
        // Arrange
        let sql = "CREATE GRAPH IF NOT EXISTS knowledge (NODES (label TEXT, embedding VECTOR(2)), EDGES (source TEXT))";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::CreateGraph(statement) = parsed.statement else {
            panic!("expected create graph statement");
        };
        assert_eq!(statement.name, "knowledge");
        assert!(statement.if_not_exists);
        assert_eq!(statement.node_fields.len(), 2);
        assert_eq!(statement.node_fields[0].name, "label");
        assert_eq!(statement.node_fields[1].data_type, DataType::Vector(2));
        assert_eq!(statement.edge_fields.len(), 1);
        assert_eq!(statement.edge_fields[0].name, "source");
    }

    #[test]
    fn should_parse_graph_table_function_source() {
        // Arrange
        let sql =
        "SELECT node_id FROM graph_expand('knowledge', 'person', 'alice', 2, 'out', 'knows', 10)";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        let QuerySource::TableFunction {
            name,
            function,
            lateral,
        } = statement.source
        else {
            panic!("expected table function source");
        };
        assert_eq!(name, "graph_expand");
        assert_eq!(function.args.len(), 7);
        assert!(!lateral);
    }

    #[test]
    fn should_parse_create_table_with_column_store_storage_mode() {
        // Arrange
        let sql = "CREATE TABLE analytics_docs (id TEXT, title TEXT) WITH (storage = column_store)";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::CreateTable(statement) = parsed.statement else {
            panic!("expected create table statement");
        };

        assert_eq!(statement.table, "analytics_docs");
        assert_eq!(statement.storage_mode, CollectionStorageMode::ColumnStore);
    }

    #[test]
    fn should_reject_create_table_with_column_indexed_storage_mode() {
        // Arrange
        let sql = "CREATE TABLE analytics_docs (id TEXT) WITH (storage = column_indexed)";

        // Act
        let parsed = parse_statement(sql);

        // Assert
        assert!(matches!(
            parsed,
            Err(err) if err.message().contains("column_indexed")
        ));
    }

    #[test]
    fn should_parse_drop_table_with_if_exists() {
        // Arrange
        let sql = "DROP TABLE IF EXISTS users";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::DropTable(statement) = parsed.statement else {
            panic!("expected drop table statement");
        };

        assert_eq!(statement.table, "users");
        assert!(statement.if_exists);
    }

    #[test]
    fn should_parse_create_schema_if_not_exists() {
        // Arrange
        let sql = "CREATE SCHEMA IF NOT EXISTS reporting";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::CreateSchema(statement) = parsed.statement else {
            panic!("expected create schema statement");
        };

        assert_eq!(statement.schema, "reporting");
        assert!(statement.if_not_exists);
    }

    #[test]
    fn should_parse_drop_schema_statement() {
        // Arrange
        let sql = "DROP SCHEMA IF EXISTS reporting";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::DropSchema(statement) = parsed.statement else {
            panic!("expected drop schema statement");
        };

        assert_eq!(statement.schema, "reporting");
        assert!(statement.if_exists);
    }

    #[test]
    fn should_parse_alter_schema_rename_statement() {
        // Arrange
        let sql = "ALTER SCHEMA reporting RENAME TO reporting_archive";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::AlterSchema(statement) = parsed.statement else {
            panic!("expected alter schema statement");
        };

        match statement.operation {
            cassie::sql::ast::AlterSchemaOperation::RenameTo { schema } => {
                assert_eq!(schema, "reporting_archive");
            }
        }
    }

    #[test]
    fn should_reject_create_schema_when_schema_exists_without_if_not_exists() {
        // Arrange
        let cassie =
            Cassie::new_with_data_dir(format!("/tmp/cassie-parser-schema-{}", Uuid::new_v4()))
                .unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        cassie
            .catalog
            .register_namespace("reporting", None);

        // Act
        let parsed = parse_statement("CREATE SCHEMA reporting")
            .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        // Assert
        assert!(matches!(bound, Err(err) if err.to_string().contains("namespace 'reporting' already exists")));
    });
    }

    #[test]
    fn should_parse_rename_table_alter_statement() {
        // Arrange
        let sql = "ALTER TABLE docs RENAME TO docs_archive";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::AlterTable(statement) = parsed.statement else {
            panic!("expected alter table statement");
        };

        assert_eq!(statement.table, "docs");
        match statement.operation {
            cassie::sql::ast::AlterTableOperation::RenameTo { table } => {
                assert_eq!(table, "docs_archive");
            }
            _ => panic!("expected rename operation"),
        }
    }

    #[test]
    fn should_parse_rename_column_alter_statement() {
        // Arrange
        let sql = "ALTER TABLE docs RENAME COLUMN title TO headline";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::AlterTable(statement) = parsed.statement else {
            panic!("expected alter table statement");
        };

        assert_eq!(statement.table, "docs");
        match statement.operation {
            cassie::sql::ast::AlterTableOperation::RenameColumn { from, to } => {
                assert_eq!(from, "title");
                assert_eq!(to, "headline");
            }
            _ => panic!("expected rename column operation"),
        }
    }

    #[test]
    fn should_reject_duplicate_fields_in_create_table_definition() {
        // Arrange
        let sql = "CREATE TABLE dup_cols (id TEXT, id TEXT)";

        // Act
        let parsed = parse_statement(sql);

        // Assert
        assert!(parsed.is_err());
    }

    #[test]
    fn should_parse_create_table_field_constraints() {
        // Arrange
        let sql =
        "CREATE TABLE users (id INT PRIMARY KEY, email TEXT NOT NULL UNIQUE DEFAULT 'anon', score INT CHECK (score >= 0))";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::CreateTable(statement) = parsed.statement else {
            panic!("expected create table statement");
        };
        assert_eq!(statement.table, "users");
        assert_eq!(statement.fields.len(), 3);

        assert!(statement.fields[0]
            .constraints
            .iter()
            .any(|c| c.primary_key));
        assert!(!statement.fields[0].constraints.iter().any(|c| c.unique));

        let email_constraints = &statement.fields[1].constraints;
        assert_eq!(email_constraints.len(), 1);
        assert!(email_constraints[0].not_null);
        assert!(email_constraints[0].unique);
        assert_eq!(
            email_constraints[0].default_value,
            Some(serde_json::Value::String("anon".to_string()))
        );

        let score_constraints = &statement.fields[2].constraints;
        assert_eq!(score_constraints.len(), 1);
        assert!(score_constraints[0].check.is_some());
    }

    #[test]
    fn should_parse_create_table_foreign_key_constraint() {
        // Arrange
        let sql = "CREATE TABLE orders (customer_id INT REFERENCES customers(id))";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::CreateTable(statement) = parsed.statement else {
            panic!("expected create table statement");
        };
        let constraints = &statement.fields[0].constraints;
        assert_eq!(constraints.len(), 1);
        assert_eq!(
            constraints[0].references_table.as_deref(),
            Some("customers")
        );
        assert_eq!(constraints[0].references_field.as_deref(), Some("id"));
    }

    #[test]
    fn should_reject_create_table_constraints_without_parentheses() {
        // Arrange
        let sql = "CREATE TABLE broken (id INT CHECK score >= 0)";

        // Act
        let parsed = parse_statement(sql);

        // Assert
        assert!(parsed.is_err());
    }

    #[test]
    fn should_reject_reserved_namespace_on_create_schema() {
        // Arrange
        let cassie = Cassie::new_with_data_dir(format!(
            "/tmp/cassie-parser-reserved-schema-{}",
            Uuid::new_v4()
        ))
        .unwrap();

        // Act
        let parsed = parse_statement("CREATE SCHEMA public").expect("parse should succeed");

        // Assert
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

            assert!(bound.is_err());
        });
    }

    #[test]
    fn should_reject_reserved_namespace_on_drop_schema() {
        // Arrange
        let cassie = Cassie::new_with_data_dir(format!(
            "/tmp/cassie-parser-reserved-drop-schema-{}",
            Uuid::new_v4()
        ))
        .unwrap();

        // Act
        let parsed = parse_statement("DROP SCHEMA public").expect("parse should succeed");

        // Assert
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

            assert!(bound.is_err());
        });
    }

    #[test]
    fn should_reject_reserved_namespace_on_alter_schema() {
        // Arrange
        let cassie = Cassie::new_with_data_dir(format!(
            "/tmp/cassie-parser-reserved-alter-schema-{}",
            Uuid::new_v4()
        ))
        .unwrap();

        // Act
        let parsed =
            parse_statement("ALTER SCHEMA public RENAME TO archive").expect("parse should succeed");

        // Assert
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

            assert!(bound.is_err());
        });
    }

    #[test]
    fn should_reject_create_table_when_collection_exists_without_if_not_exists() {
        // Arrange
        let cassie =
            Cassie::new_with_data_dir(format!("/tmp/cassie-parser-{}", Uuid::new_v4())).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            cassie.register_collection(
                "existing_table".to_string(),
                Schema {
                    fields: vec![FieldSchema {
                        name: "id".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    }],
                },
            );

            // Act
            let parsed = parse_statement("CREATE TABLE existing_table (title TEXT)")
                .expect("parse should succeed");
            let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

            // Assert
            assert!(bound.is_err());
        });
    }

    #[test]
    fn should_reject_drop_table_when_collection_missing_without_if_exists() {
        // Arrange
        let cassie =
            Cassie::new_with_data_dir(format!("/tmp/cassie-parser-{}", Uuid::new_v4())).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            // Act
            let parsed = parse_statement("DROP TABLE missing_table").expect("parse should succeed");
            let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

            // Assert
            assert!(bound.is_err());
        });
    }
}

// Formerly tests/parser_database_scope.rs.
mod parser_database_scope {
    use cassie::app::CassieError;
    use cassie::catalog::{canonical_relation_name, canonical_schema_name, Catalog};
    use cassie::sql::ast::{QuerySource, QueryStatement};
    use cassie::sql::binder::{bind_with_context, BindingContext};
    use cassie::sql::parse_statement;
    use cassie::types::{DataType, FieldSchema, Schema};

    #[test]
    fn should_parse_create_plus_drop_database_statements() {
        // Arrange
        let create_sql = "CREATE DATABASE tenant_b";
        let drop_sql = "DROP DATABASE IF EXISTS tenant_b";

        // Act
        let create = parse_statement(create_sql).expect("create database should parse");
        let drop = parse_statement(drop_sql).expect("drop database should parse");

        // Assert
        assert!(matches!(
            create.statement,
            QueryStatement::CreateDatabase(_)
        ));
        assert!(matches!(drop.statement, QueryStatement::DropDatabase(_)));
    }

    #[test]
    fn should_bind_unqualified_names_through_search_path() {
        // Arrange
        let catalog = Catalog::new();
        catalog.register_database("postgres", None);
        catalog.register_namespace(&canonical_schema_name("postgres", "public"), None);
        catalog.register_namespace(&canonical_schema_name("postgres", "reporting"), None);
        catalog.register_collection(
            &canonical_relation_name("postgres", "public", "orders"),
            Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            }
            .fields
            .into_iter()
            .map(|field| (field.name, field.data_type))
            .collect(),
        );
        catalog.register_collection(
            &canonical_relation_name("postgres", "reporting", "orders"),
            vec![("title".to_string(), DataType::Text)],
        );
        let context = BindingContext::scoped(
            "postgres",
            vec!["reporting".to_string(), "public".to_string()],
        );
        let parsed = parse_statement("SELECT title FROM orders").expect("select should parse");

        // Act
        let bound = bind_with_context(parsed, &catalog, &context).expect("bind should succeed");

        // Assert
        let QueryStatement::Select(select) = bound.statement.statement else {
            panic!("expected SELECT");
        };
        let QuerySource::Collection(name) = select.source else {
            panic!("expected collection source");
        };
        assert_eq!(
            name,
            canonical_relation_name("postgres", "reporting", "orders")
        );
    }

    #[test]
    fn should_reject_cross_database_relation_references() {
        // Arrange
        let catalog = Catalog::new();
        let context = BindingContext::scoped("postgres", vec!["public".to_string()]);
        let parsed = parse_statement("SELECT title FROM tenant_b.public.orders")
            .expect("select should parse");

        // Act
        let error = bind_with_context(parsed, &catalog, &context)
            .expect_err("cross-database bind should fail");

        // Assert
        let CassieError::Unsupported(message) = error else {
            panic!("expected unsupported cross-database error");
        };
        assert!(message.contains("cross-database relation references are not supported"));
    }
}

// Formerly tests/parser_dml_transactions.rs.
mod parser_dml_transactions {
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
}

// Formerly tests/parser_expressions.rs.
mod parser_expressions {
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
    fn should_parse_pgvector_cosine_ordering() {
        // Arrange
        let sql = "SELECT * FROM docs ORDER BY embedding <=> $1 LIMIT 5";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        let order = &statement.order;
        assert_eq!(order.len(), 1);
        let expr = &order[0].expr;
        match expr {
            Expr::Binary {
                op: BinaryOp::PgvectorCosine,
                ..
            } => {}
            _ => panic!("expected pgvector cosine order operator"),
        }
        assert_eq!(statement.limit, Some(5));
    }

    #[test]
    fn should_parse_pgvector_dot_ordering() {
        // Arrange
        let sql = "SELECT * FROM docs ORDER BY embedding <#> $1 LIMIT 5";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        let expr = &statement.order[0].expr;
        match expr {
            Expr::Binary {
                op: BinaryOp::PgvectorDot,
                ..
            } => {}
            _ => panic!("expected pgvector dot order operator"),
        }
    }

    #[test]
    fn should_parse_pgvector_l2_ordering() {
        // Arrange
        let sql = "SELECT * FROM docs ORDER BY embedding <-> $1 LIMIT 5";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        let expr = &statement.order[0].expr;
        match expr {
            Expr::Binary {
                op: BinaryOp::PgvectorL2,
                ..
            } => {}
            _ => panic!("expected pgvector l2 order operator"),
        }
    }

    #[test]
    fn should_parse_vector_function_argument_with_commas() {
        // Arrange
        let sql = "SELECT vector_score(embedding, '[1,0]') FROM docs";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        let projection = &statement.projection[0];
        match projection {
            SelectItem::Function { function, .. } => {
                assert_eq!(function.name, "vector_score");
                assert_eq!(function.args.len(), 2);
                assert!(
                    matches!(function.args[0], Expr::Column(ref column) if column == "embedding")
                );
                assert!(
                    matches!(&function.args[1], Expr::StringLiteral(value) if value == "[1,0]")
                );
            }
            _ => panic!("expected vector function"),
        }
    }

    #[test]
    fn should_parse_boolean_precedence_in_where_clause() {
        // Arrange
        let sql = "SELECT * FROM docs WHERE title = 'alpha' OR title = 'beta' AND active = true";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        let filter = statement.filter.expect("filter expected");

        let Expr::Binary {
            op: or_op,
            left: left_expr,
            right: right_expr,
        } = filter
        else {
            panic!("expected binary filter");
        };
        assert!(matches!(or_op, BinaryOp::Or));

        match left_expr.as_ref() {
            Expr::Binary {
                op: BinaryOp::Eq, ..
            } => {}
            _ => panic!("expected OR left-side equality"),
        }

        match right_expr.as_ref() {
            Expr::Binary {
                op: BinaryOp::And, ..
            } => {}
            _ => panic!("expected OR right-side conjunction"),
        }
    }

    #[test]
    fn should_parse_parenthesized_where_changes_precedence() {
        // Arrange
        let sql = "SELECT * FROM docs WHERE (title = 'alpha' OR title = 'beta') AND active = true";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        let filter = statement.filter.expect("filter expected");

        let Expr::Binary {
            op: and_op,
            left,
            right,
        } = filter
        else {
            panic!("expected binary filter");
        };
        assert!(matches!(and_op, BinaryOp::And));

        match left.as_ref() {
            Expr::Binary {
                op: BinaryOp::Or, ..
            } => {}
            _ => panic!("expected grouped OR on the left side"),
        }

        match right.as_ref() {
            Expr::Binary {
                op: BinaryOp::Eq, ..
            } => {}
            _ => panic!("expected active = true predicate on right side"),
        }
    }

    #[test]
    fn should_reject_negative_offset() {
        // Arrange
        let sql = "SELECT * FROM docs ORDER BY id OFFSET -1";

        // Act
        let parsed = parse_statement(sql);

        // Assert
        assert!(parsed.is_err());
    }

    #[test]
    fn should_reject_negative_limit() {
        // Arrange
        let sql = "SELECT * FROM docs LIMIT -5";

        // Act
        let parsed = parse_statement(sql);

        // Assert
        assert!(parsed.is_err());
    }

    #[test]
    fn should_parse_parameter_positions() {
        // Arrange
        let sql = "SELECT * FROM docs WHERE title = $2 AND id = $1";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        let filter = statement.filter.expect("filter expected");

        let Expr::Binary { left, right, op: _ } = filter else {
            panic!("expected parameterized filter");
        };

        let (left_left, left_right) = match left.as_ref() {
            Expr::Binary {
                op: BinaryOp::Eq,
                left,
                right,
                ..
            } => (left.as_ref(), right.as_ref()),
            _ => panic!("expected lhs parameterized equality"),
        };

        assert!(matches!(left_left, Expr::Column(_)));
        assert!(matches!(left_right, Expr::Param(1)));

        let (right_left, right_right) = match right.as_ref() {
            Expr::Binary {
                op: BinaryOp::Eq,
                left,
                right,
                ..
            } => (left.as_ref(), right.as_ref()),
            _ => panic!("expected rhs parameterized equality"),
        };

        assert!(matches!(right_left, Expr::Column(_)));
        assert!(matches!(right_right, Expr::Param(0)));
    }

    #[test]
    fn should_parse_table_free_literal_parameter_projection() {
        // Arrange
        let sql = "SELECT 1 AS one, NULL AS missing, $1::INT AS value";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        assert!(matches!(statement.source, QuerySource::SingleRow));
        assert!(matches!(statement.projection[0], SelectItem::Expr { .. }));
        assert!(matches!(statement.projection[1], SelectItem::Expr { .. }));
        assert!(matches!(statement.projection[2], SelectItem::Expr { .. }));
    }

    #[test]
    fn should_parse_is_null_predicate() {
        // Arrange
        let sql = "SELECT title FROM docs WHERE archived_at IS NULL";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        assert!(matches!(
            statement.filter,
            Some(Expr::IsNull { negated: false, .. })
        ));
    }

    #[test]
    fn should_parse_in_list_predicate() {
        // Arrange
        let sql = "SELECT title FROM docs WHERE title IN ('alpha', 'beta')";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        assert!(matches!(
            statement.filter,
            Some(Expr::InList { negated: false, .. })
        ));
    }

    #[test]
    fn should_parse_between_predicate() {
        // Arrange
        let sql = "SELECT title FROM docs WHERE score BETWEEN 10 AND 20";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        assert!(matches!(
            statement.filter,
            Some(Expr::Between { negated: false, .. })
        ));
    }

    fn select_filter(sql: &str) -> Expr {
        let QueryStatement::Select(statement) = parse_statement(sql)
            .expect("parse should succeed")
            .statement
        else {
            panic!("expected select statement");
        };
        statement.filter.expect("filter should exist")
    }

    fn is_between(expr: &Expr, expected_negated: bool) -> bool {
        matches!(expr, Expr::Between { negated, .. } if *negated == expected_negated)
    }

    #[test]
    fn should_bind_between_tighter_than_a_following_conjunct() {
        // Arrange
        let sql = "SELECT a FROM t WHERE a BETWEEN 1 AND 3 AND b = 'v1'";

        // Act
        let filter = select_filter(sql);

        // Assert
        let Expr::Binary {
            left,
            op: BinaryOp::And,
            right,
        } = filter
        else {
            panic!("expected AND at the top of the filter");
        };
        assert!(is_between(&left, false));
        assert!(matches!(
            *right,
            Expr::Binary {
                op: BinaryOp::Eq,
                ..
            }
        ));
    }

    #[test]
    fn should_bind_between_tighter_than_a_preceding_conjunct() {
        // Arrange
        let sql = "SELECT a FROM t WHERE b = 'v1' AND a BETWEEN 1 AND 3";

        // Act
        let filter = select_filter(sql);

        // Assert
        let Expr::Binary {
            left,
            op: BinaryOp::And,
            right,
        } = filter
        else {
            panic!("expected AND at the top of the filter");
        };
        assert!(matches!(
            *left,
            Expr::Binary {
                op: BinaryOp::Eq,
                ..
            }
        ));
        assert!(is_between(&right, false));
    }

    #[test]
    fn should_split_conjunction_chains_mixing_between_with_not_between() {
        // Arrange
        let sql = "SELECT a FROM t WHERE a BETWEEN 1 AND 3 AND b NOT BETWEEN 4 AND 6 AND c = 1";

        // Act
        let filter = select_filter(sql);

        // Assert
        let Expr::Binary {
            left,
            op: BinaryOp::And,
            right,
        } = filter
        else {
            panic!("expected AND at the top of the filter");
        };
        assert!(is_between(&left, false));
        let Expr::Binary {
            left: middle,
            op: BinaryOp::And,
            right: last,
        } = *right
        else {
            panic!("expected nested AND");
        };
        assert!(is_between(&middle, true));
        assert!(matches!(
            *last,
            Expr::Binary {
                op: BinaryOp::Eq,
                ..
            }
        ));
    }

    #[test]
    fn should_ignore_nested_or_quoted_between_when_splitting_conjunctions() {
        // Arrange
        let sql = "SELECT a FROM t WHERE (b BETWEEN 4 AND 6 OR c = ' between x and ') AND CASE WHEN d BETWEEN 1 AND 2 THEN true ELSE false END AND a BETWEEN 1 AND 3";

        // Act
        let filter = select_filter(sql);

        // Assert
        let Expr::Binary {
            left,
            op: BinaryOp::And,
            right,
        } = filter
        else {
            panic!("expected AND at the top of the filter");
        };
        assert!(matches!(
            *left,
            Expr::Binary {
                op: BinaryOp::Or,
                ..
            }
        ));
        let Expr::Binary {
            left: middle,
            op: BinaryOp::And,
            right: last,
        } = *right
        else {
            panic!("expected nested AND");
        };
        assert!(matches!(*middle, Expr::Case { .. }));
        assert!(is_between(&last, false));
    }

    #[test]
    fn should_bind_between_tighter_than_conjunctions_in_delete_filters() {
        // Arrange
        let sql = "DELETE FROM t WHERE a BETWEEN 1 AND 3 AND b = 'v1'";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Delete(statement) = parsed.statement else {
            panic!("expected delete statement");
        };
        let Some(Expr::Binary {
            left,
            op: BinaryOp::And,
            ..
        }) = statement.filter
        else {
            panic!("expected AND at the top of the filter");
        };
        assert!(is_between(&left, false));
    }

    #[test]
    fn should_parse_cast_function_expression() {
        // Arrange
        let sql = "SELECT title FROM docs WHERE CAST(score AS TEXT) = '10'";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        let Some(Expr::Binary { left, .. }) = statement.filter else {
            panic!("expected binary predicate");
        };
        assert!(matches!(*left, Expr::Cast { .. }));
    }

    #[test]
    fn should_parse_postgres_style_cast_expression() {
        // Arrange
        let sql = "SELECT title FROM docs WHERE score::TEXT = '10'";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        let Some(Expr::Binary { left, .. }) = statement.filter else {
            panic!("expected binary predicate");
        };
        assert!(matches!(*left, Expr::Cast { .. }));
    }

    #[test]
    fn should_parse_order_by_nulls_last() {
        // Arrange
        let sql = "SELECT title FROM docs ORDER BY archived_at ASC NULLS LAST";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        assert!(matches!(
            statement.order[0].nulls,
            Some(cassie::sql::ast::NullsOrder::Last)
        ));
    }

    #[test]
    fn should_parse_exists_predicate() {
        // Arrange
        let sql = "SELECT title FROM docs WHERE EXISTS (SELECT title FROM archive)";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        assert!(matches!(statement.filter, Some(Expr::Exists(_))));
    }

    #[test]
    fn should_parse_not_exists_predicate() {
        // Arrange
        let sql = "SELECT title FROM docs WHERE NOT EXISTS (SELECT title FROM archived_docs)";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        assert!(matches!(
            statement.filter,
            Some(Expr::Not { ref expr }) if matches!(expr.as_ref(), Expr::Exists(_))
        ));
    }

    #[test]
    fn should_reject_not_without_expression() {
        // Arrange
        let sql = "SELECT title FROM docs WHERE NOT";

        // Act
        let error = parse_statement(sql).expect_err("NOT without an expression should fail");

        // Assert
        assert_eq!(error.kind(), cassie::sql::SqlErrorKind::Syntax);
        assert!(error.message().contains("NOT requires an expression"));
    }

    #[test]
    fn should_reject_empty_in_list_predicate() {
        // Arrange
        let sql = "SELECT title FROM docs WHERE title IN ()";

        // Act
        let error = parse_statement(sql).expect_err("empty IN predicate should fail");

        // Assert
        assert_eq!(error.kind(), cassie::sql::SqlErrorKind::Syntax);
        assert!(error.message().contains("IN predicate"));
    }

    #[test]
    fn should_reject_exists_without_select_subquery() {
        // Arrange
        let sql = "SELECT title FROM docs WHERE EXISTS (INSERT INTO docs (title) VALUES ('x'))";

        // Act
        let error = parse_statement(sql).expect_err("EXISTS without SELECT subquery should fail");

        // Assert
        assert_eq!(error.kind(), cassie::sql::SqlErrorKind::Syntax);
        assert!(error.message().contains("EXISTS requires"));
    }
}

// Delimited (double-quoted) identifiers in expression, DML-target and alias positions.
mod sql_quoted_identifiers {
    use super::support_sql as support;

    use cassie::app::{Cassie, CassieSession};
    use cassie::executor::QueryResult;
    use cassie::sql::ast::{QueryStatement, SelectItem};
    use cassie::sql::parse_statement;
    use cassie::types::Value;

    use support::{data_dir, use_local_storage};

    fn with_users_table(label: &str, test: impl FnOnce(&Cassie, &CassieSession)) {
        use_local_storage();
        let path = data_dir(label);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
            cassie.startup().expect("start Cassie");
            let session = cassie.create_session("tester", None);
            for sql in [
                "CREATE TABLE users (id BIGINT, name TEXT)",
                "INSERT INTO users (id, name) VALUES (1, 'alice')",
                "INSERT INTO users (id, name) VALUES (2, 'bob')",
            ] {
                cassie
                    .execute_sql(&session, sql, vec![])
                    .expect("seed users table");
            }
            test(&cassie, &session);
        });
        let _ = std::fs::remove_dir_all(path);
    }

    fn run(cassie: &Cassie, session: &CassieSession, sql: &str) -> QueryResult {
        match cassie.execute_sql(session, sql, vec![]) {
            Ok(result) => result,
            Err(error) => panic!("statement failed: {sql}: {error}"),
        }
    }

    fn string_rows(values: &[&str]) -> Vec<Vec<Value>> {
        values
            .iter()
            .map(|value| vec![Value::String((*value).to_string())])
            .collect()
    }

    fn create_case_distinct_fulltext_fixture(cassie: &Cassie, session: &CassieSession) {
        run(
            cassie,
            session,
            "CREATE TABLE fulltext_case_fields (id INT, \"body\" TEXT, \"Body\" TEXT)",
        );
        run(
            cassie,
            session,
            "INSERT INTO fulltext_case_fields (id, \"body\", \"Body\") VALUES \
             (1, 'amber amber', 'violet'), (2, 'violet', 'amber amber'), \
             (3, 'amber amber', 'amber amber'), \
             (4, 'amber amber amber amber', 'amber amber amber amber')",
        );
        run(
            cassie,
            session,
            "CREATE INDEX fulltext_lower_body ON fulltext_case_fields \
             USING fulltext (\"body\") WITH (boost = 1.0)",
        );
        run(
            cassie,
            session,
            "CREATE INDEX fulltext_upper_body ON fulltext_case_fields \
             USING fulltext (\"Body\") WITH (boost = 3.0)",
        );
    }

    fn create_case_distinct_dml_fixture(cassie: &Cassie, session: &CassieSession) {
        run(
            cassie,
            session,
            "CREATE TABLE separate_case (\"a\" INT, \"A\" INT)",
        );
        run(
            cassie,
            session,
            "INSERT INTO separate_case (\"a\", \"A\") VALUES (1, 2)",
        );
    }

    #[test]
    fn should_resolve_a_quoted_identifier_in_where_to_the_column() {
        with_users_table("quoted_ident_where", |cassie, session| {
            // Arrange
            let sql = "SELECT name FROM users WHERE \"name\" = 'alice'";

            // Act
            let selected = run(cassie, session, sql);

            // Assert
            assert_eq!(selected.rows, string_rows(&["alice"]));
        });
    }

    #[test]
    fn should_project_a_quoted_identifier_as_the_column_value() {
        with_users_table("quoted_ident_projection", |cassie, session| {
            // Arrange
            let sql = "SELECT \"name\" FROM users ORDER BY \"id\" DESC";

            // Act
            let selected = run(cassie, session, sql);

            // Assert
            assert_eq!(selected.columns[0].name, "name");
            assert_eq!(selected.rows, string_rows(&["bob", "alice"]));
        });
    }

    #[test]
    fn should_update_a_quoted_target_on_rows_matched_by_a_quoted_identifier() {
        with_users_table("quoted_ident_update", |cassie, session| {
            // Arrange
            let update = "UPDATE users SET \"name\" = 'x' WHERE \"id\" = 1";

            // Act
            let updated = run(cassie, session, update);
            let remaining = run(cassie, session, "SELECT name FROM users ORDER BY id");

            // Assert
            assert_eq!(updated.command, "UPDATE 1");
            assert_eq!(remaining.rows, string_rows(&["x", "bob"]));
        });
    }

    #[test]
    fn should_delete_rows_matched_by_a_quoted_identifier() {
        with_users_table("quoted_ident_delete", |cassie, session| {
            // Arrange
            let delete = "DELETE FROM users WHERE \"id\" = 2";

            // Act
            let deleted = run(cassie, session, delete);
            let remaining = run(cassie, session, "SELECT name FROM users ORDER BY id");

            // Assert
            assert_eq!(deleted.command, "DELETE 1");
            assert_eq!(remaining.rows, string_rows(&["alice"]));
        });
    }

    #[test]
    fn should_insert_into_a_quoted_column_created_by_create_table() {
        with_users_table("quoted_ident_insert_columns", |cassie, session| {
            // Arrange
            run(cassie, session, "CREATE TABLE q (\"Col\" TEXT)");

            // Act
            let inserted = run(cassie, session, "INSERT INTO q (\"Col\") VALUES ('v')");
            let selected = run(cassie, session, "SELECT \"Col\" FROM q");

            // Assert
            assert_eq!(inserted.command, "INSERT 0 1");
            assert_eq!(selected.rows, string_rows(&["v"]));
        });
    }

    #[test]
    fn should_strip_quotes_from_a_quoted_column_alias() {
        with_users_table("quoted_ident_alias", |cassie, session| {
            // Arrange
            let sql = "SELECT name AS \"Foo\" FROM users WHERE id = 1";

            // Act
            let selected = run(cassie, session, sql);

            // Assert
            assert_eq!(selected.columns[0].name, "Foo");
            assert_eq!(selected.rows, string_rows(&["alice"]));
        });
    }

    #[test]
    fn should_resolve_case_distinct_delimited_projection_aliases_exactly() {
        with_users_table("quoted_ident_case_distinct_aliases", |cassie, session| {
            // Arrange
            run(
                cassie,
                session,
                "CREATE TABLE alias_case_distinct (\"a\" INT, \"A\" INT)",
            );
            run(
                cassie,
                session,
                "INSERT INTO alias_case_distinct (\"a\", \"A\") VALUES (2, 1), (1, 2)",
            );

            // Act
            let upper_alias = run(
                cassie,
                session,
                "SELECT \"a\" AS \"x\", \"A\" AS \"X\" \
                 FROM alias_case_distinct ORDER BY \"X\"",
            );
            let upper_ordinal = run(
                cassie,
                session,
                "SELECT \"a\" AS \"x\", \"A\" AS \"X\" \
                 FROM alias_case_distinct ORDER BY 2",
            );
            let grouped_upper_alias = run(
                cassie,
                session,
                "SELECT \"a\" AS \"x\", \"A\" AS \"X\", count(*) \
                 FROM alias_case_distinct GROUP BY \"a\", \"A\" ORDER BY \"X\"",
            );
            let lower_alias = run(
                cassie,
                session,
                "SELECT \"a\" AS \"x\", \"A\" AS \"X\" \
                 FROM alias_case_distinct ORDER BY \"x\"",
            );

            // Assert
            let ordered_by_upper = vec![
                vec![Value::Int64(2), Value::Int64(1)],
                vec![Value::Int64(1), Value::Int64(2)],
            ];
            assert_eq!(upper_alias.rows, ordered_by_upper);
            assert_eq!(upper_ordinal.rows, ordered_by_upper);
            assert_eq!(
                grouped_upper_alias.rows,
                vec![
                    vec![Value::Int64(2), Value::Int64(1), Value::Int64(1)],
                    vec![Value::Int64(1), Value::Int64(2), Value::Int64(1)],
                ]
            );
            assert_eq!(
                lower_alias.rows,
                vec![
                    vec![Value::Int64(1), Value::Int64(2)],
                    vec![Value::Int64(2), Value::Int64(1)],
                ]
            );
            assert_eq!(
                upper_alias
                    .columns
                    .iter()
                    .map(|column| column.name.as_str())
                    .collect::<Vec<_>>(),
                ["x", "X"]
            );
        });
    }

    #[test]
    fn should_preserve_case_distinct_delimited_cte_aliases() {
        with_users_table(
            "quoted_ident_case_distinct_cte_aliases",
            |cassie, session| {
                // Arrange
                run(
                    cassie,
                    session,
                    "CREATE TABLE cte_alias_case_distinct (\"a\" INT, \"A\" INT)",
                );
                run(
                    cassie,
                    session,
                    "INSERT INTO cte_alias_case_distinct (\"a\", \"A\") VALUES (2, 1), (1, 2)",
                );

                // Act
                let selected = run(
                    cassie,
                    session,
                    "WITH named (\"x\", \"X\") AS \
                 (SELECT \"a\", \"A\" FROM cte_alias_case_distinct) \
                 SELECT \"x\", \"X\" FROM named ORDER BY \"X\"",
                );

                // Assert
                assert_eq!(
                    selected.rows,
                    vec![
                        vec![Value::Int64(2), Value::Int64(1)],
                        vec![Value::Int64(1), Value::Int64(2)],
                    ]
                );
                assert_eq!(
                    selected
                        .columns
                        .iter()
                        .map(|column| column.name.as_str())
                        .collect::<Vec<_>>(),
                    ["x", "X"]
                );
            },
        );
    }

    #[test]
    fn should_fold_undelimited_column_aliases() {
        // Arrange
        let parsed = parse_statement("SELECT 1 AS Total").expect("parse projection alias");
        let QueryStatement::Select(select) = parsed.statement else {
            panic!("expected SELECT statement");
        };

        // Act
        let aliases = select
            .projection
            .iter()
            .map(|item| match item {
                SelectItem::Expr { alias, .. } => alias.as_deref(),
                _ => None,
            })
            .collect::<Vec<_>>();

        // Assert
        assert_eq!(aliases, vec![Some("total")]);
    }

    #[test]
    fn should_preserve_delimited_column_alias_spelling() {
        // Arrange
        let parsed = parse_statement("SELECT 1 AS \"Exact\"").expect("parse projection alias");
        let QueryStatement::Select(select) = parsed.statement else {
            panic!("expected SELECT statement");
        };

        // Act
        let aliases = select
            .projection
            .iter()
            .map(|item| match item {
                SelectItem::Expr { alias, .. } => alias.as_deref(),
                _ => None,
            })
            .collect::<Vec<_>>();

        // Assert
        assert_eq!(aliases, vec![Some("Exact")]);
    }

    #[test]
    fn should_arbitrate_on_conflict_with_a_quoted_target_column() {
        with_users_table("quoted_ident_on_conflict", |cassie, session| {
            // Arrange
            run(
                cassie,
                session,
                "CREATE TABLE keyed (id BIGINT PRIMARY KEY, v TEXT)",
            );
            run(cassie, session, "INSERT INTO keyed (id, v) VALUES (1, 'a')");

            // Act
            let upserted = run(
                cassie,
                session,
                "INSERT INTO keyed (\"id\", \"v\") VALUES (1, 'b') ON CONFLICT (\"id\") DO UPDATE SET \"v\" = 'c'",
            );
            let selected = run(cassie, session, "SELECT v FROM keyed");

            // Assert
            assert_eq!(upserted.command, "INSERT 0 1");
            assert_eq!(selected.rows, string_rows(&["c"]));
        });
    }

    #[test]
    fn should_update_case_distinct_columns_from_the_matching_excluded_fields() {
        with_users_table("quoted_ident_on_conflict_excluded", |cassie, session| {
            // Arrange
            run(
                cassie,
                session,
                "CREATE TABLE conflict_case_distinct (\"key\" TEXT UNIQUE, \"a\" TEXT, \"A\" TEXT)",
            );
            run(
                cassie,
                session,
                "INSERT INTO conflict_case_distinct (\"key\", \"a\", \"A\") VALUES ('k', 'lower-old', 'upper-old')",
            );

            // Act
            run(
                cassie,
                session,
                "INSERT INTO conflict_case_distinct (\"key\", \"a\", \"A\") VALUES ('k', 'lower-new', 'upper-new') \
                 ON CONFLICT (\"key\") DO UPDATE SET \"a\" = excluded.\"a\", \"A\" = excluded.\"A\"",
            );
            let selected = run(
                cassie,
                session,
                "SELECT \"a\", \"A\" FROM conflict_case_distinct",
            );

            // Assert
            assert_eq!(
                selected.rows,
                vec![vec![
                    Value::String("lower-new".to_string()),
                    Value::String("upper-new".to_string()),
                ]]
            );
        });
    }

    #[test]
    fn should_group_by_a_quoted_identifier_as_the_column_value() {
        with_users_table("quoted_ident_group", |cassie, session| {
            // Arrange
            run(
                cassie,
                session,
                "INSERT INTO users (id, name) VALUES (3, 'bob')",
            );
            let sql = "SELECT \"name\", count(*) FROM users GROUP BY \"name\" ORDER BY \"name\"";

            // Act
            let grouped = run(cassie, session, sql);

            // Assert
            assert_eq!(
                grouped.rows,
                vec![
                    vec![Value::String("alice".to_string()), Value::Int64(1)],
                    vec![Value::String("bob".to_string()), Value::Int64(2)],
                ]
            );
        });
    }

    #[test]
    fn should_compare_a_quoted_identifier_against_a_numeric_literal() {
        with_users_table("quoted_ident_compare", |cassie, session| {
            // Arrange
            let sql = "SELECT name FROM users WHERE \"id\" > 1";

            // Act
            let compared = run(cassie, session, sql);

            // Assert
            assert_eq!(compared.rows, string_rows(&["bob"]));
        });
    }

    #[test]
    fn should_bind_a_parameter_against_a_quoted_indexed_column() {
        with_users_table("quoted_ident_indexed_param", |cassie, session| {
            // Arrange
            run(cassie, session, "CREATE INDEX users_id_idx ON users (id)");
            let sql = "SELECT \"name\" FROM users WHERE \"id\" = $1";

            // Act
            let selected = match cassie.execute_sql(session, sql, vec![Value::Int64(2)]) {
                Ok(result) => result,
                Err(error) => panic!("statement failed: {sql}: {error}"),
            };

            // Assert
            assert_eq!(selected.rows, string_rows(&["bob"]));
        });
    }

    #[test]
    fn should_resolve_quoted_identifiers_containing_spaces_or_multibyte_characters() {
        with_users_table("quoted_ident_spaces", |cassie, session| {
            // Arrange
            run(
                cassie,
                session,
                "CREATE TABLE people (\"first name\" TEXT, \"café\" TEXT)",
            );
            run(
                cassie,
                session,
                "INSERT INTO people (\"first name\", \"café\") VALUES ('ada', 'noir')",
            );
            let sql = "SELECT \"café\" FROM people WHERE \"first name\" = 'ada'";

            // Act
            let selected = run(cassie, session, sql);

            // Assert
            assert_eq!(selected.columns[0].name, "café");
            assert_eq!(selected.rows, string_rows(&["noir"]));
        });
    }

    #[test]
    fn should_parse_quoted_qualified_identifiers_as_column_paths() {
        // Arrange
        let cases = [
            ("SELECT \"users\".\"name\" FROM users", "users.name"),
            ("SELECT users.\"name\" FROM users", "users.name"),
            ("SELECT \"we\"\"ird\" FROM users", "\"we\"\"ird\""),
        ];

        // Act
        let parsed = cases.map(|(sql, expected)| {
            let statement = parse_statement(sql).expect("parse quoted identifier");
            let QueryStatement::Select(select) = statement.statement else {
                panic!("expected SELECT statement for {sql}");
            };
            (select.projection[0].clone(), expected)
        });

        // Assert
        for (item, expected) in parsed {
            let SelectItem::Column { name, .. } = item else {
                panic!("expected column projection for {expected}");
            };
            assert_eq!(name, expected);
        }
    }

    #[test]
    fn should_match_delimited_column_identifiers_exactly() {
        with_users_table("quoted_ident_exact_case", |cassie, session| {
            // Arrange
            run(cassie, session, "CREATE TABLE exact_case (\"Email\" TEXT)");
            run(
                cassie,
                session,
                "INSERT INTO exact_case (\"Email\") VALUES ('ada')",
            );

            // Act
            let exact = run(cassie, session, "SELECT \"Email\" FROM exact_case");
            let mismatched =
                cassie.execute_sql(session, "SELECT \"EMAIL\" FROM exact_case", vec![]);
            let undelimited = cassie.execute_sql(session, "SELECT email FROM exact_case", vec![]);

            // Assert
            assert_eq!(exact.rows, string_rows(&["ada"]));
            assert!(mismatched.is_err(), "quoted spelling must match exactly");
            assert!(
                undelimited.is_err(),
                "email must not resolve to stored Email"
            );
        });
    }

    #[test]
    fn should_fold_undelimited_column_identifiers_to_lowercase() {
        with_users_table("unquoted_ident_lowercase", |cassie, session| {
            // Arrange
            run(cassie, session, "CREATE TABLE folded_case (Email TEXT)");
            run(
                cassie,
                session,
                "INSERT INTO folded_case (EMAIL) VALUES ('ada')",
            );

            // Act
            let unquoted = run(cassie, session, "SELECT email FROM folded_case");
            let quoted_original_case =
                cassie.execute_sql(session, "SELECT \"Email\" FROM folded_case", vec![]);

            // Assert
            assert_eq!(unquoted.rows, string_rows(&["ada"]));
            assert!(
                quoted_original_case.is_err(),
                "unquoted declarations fold to lowercase"
            );
        });
    }

    #[test]
    fn should_keep_delimited_columns_that_differ_only_by_case() {
        with_users_table("quoted_ident_case_distinct", |cassie, session| {
            // Arrange
            run(
                cassie,
                session,
                "CREATE TABLE case_distinct (\"a\" INT, \"A\" INT)",
            );
            run(
                cassie,
                session,
                "INSERT INTO case_distinct (\"a\", \"A\") VALUES (1, 2)",
            );

            // Act
            let selected = run(cassie, session, "SELECT \"a\", \"A\" FROM case_distinct");
            let cte = run(
                cassie,
                session,
                "WITH exact AS (SELECT * FROM case_distinct) \
                 SELECT \"a\", \"A\" FROM exact",
            );

            // Assert
            let expected = vec![vec![Value::Int64(1), Value::Int64(2)]];
            assert_eq!(selected.rows, expected);
            assert_eq!(cte.rows, expected);
        });
    }

    #[test]
    fn should_resolve_case_distinct_delimited_join_columns_with_case_insensitive_qualifiers() {
        with_users_table("quoted_ident_case_distinct_join", |cassie, session| {
            // Arrange
            run(
                cassie,
                session,
                "CREATE TABLE quoted_join_left (\"a\" INT, \"A\" INT)",
            );
            run(
                cassie,
                session,
                "CREATE TABLE quoted_join_right (\"a\" INT, \"A\" INT)",
            );
            run(
                cassie,
                session,
                "INSERT INTO quoted_join_left (\"a\", \"A\") VALUES \
                 (10, 1), (11, 2), (12, 10)",
            );
            run(
                cassie,
                session,
                "INSERT INTO quoted_join_right (\"a\", \"A\") VALUES \
                 (1, 20), (2, 21), (10, 22), (20, 99)",
            );

            // Act
            let joined = run(
                cassie,
                session,
                "SELECT quoted_join_left.\"a\", QUOTED_JOIN_LEFT.\"A\", \
                 quoted_join_right.\"a\", QUOTED_JOIN_RIGHT.\"A\" \
                 FROM quoted_join_left JOIN quoted_join_right \
                 ON quoted_join_left.\"A\" = QUOTED_JOIN_RIGHT.\"a\" \
                 ORDER BY quoted_join_left.\"a\"",
            );

            // Assert
            assert_eq!(
                joined.rows,
                vec![
                    vec![
                        Value::Int64(10),
                        Value::Int64(1),
                        Value::Int64(1),
                        Value::Int64(20),
                    ],
                    vec![
                        Value::Int64(11),
                        Value::Int64(2),
                        Value::Int64(2),
                        Value::Int64(21),
                    ],
                    vec![
                        Value::Int64(12),
                        Value::Int64(10),
                        Value::Int64(10),
                        Value::Int64(22),
                    ],
                ]
            );
        });
    }

    #[test]
    fn should_score_case_distinct_delimited_text_fields_independently() {
        with_users_table("quoted_ident_case_distinct_fulltext", |cassie, session| {
            // Arrange
            create_case_distinct_fulltext_fixture(cassie, session);

            // Act
            let scored = run(
                cassie,
                session,
                "SELECT id, search_score(\"body\", 'amber') AS lower_score, \
                 search_score(\"Body\", 'amber') AS upper_score \
                 FROM fulltext_case_fields ORDER BY id",
            );

            // Assert
            assert_eq!(scored.rows.len(), 4);
            assert_eq!(scored.rows[0][0], Value::Int64(1));
            assert!(matches!(scored.rows[0][1], Value::Float64(score) if score > 0.0));
            assert_eq!(scored.rows[0][2], Value::Float64(0.0));
            assert_eq!(scored.rows[1][0], Value::Int64(2));
            assert_eq!(scored.rows[1][1], Value::Float64(0.0));
            assert!(
                matches!(scored.rows[1][2], Value::Float64(score) if score > 0.0),
                "expected the exact Body score to match amber"
            );
            let Value::Float64(lower_score) = scored.rows[2][1] else {
                panic!("expected lower-field full-text score");
            };
            let Value::Float64(upper_score) = scored.rows[2][2] else {
                panic!("expected upper-field full-text score");
            };
            assert!(lower_score > 0.0);
            assert!((upper_score - lower_score * 3.0).abs() <= f64::EPSILON);
        });
    }

    #[test]
    fn should_search_case_distinct_delimited_text_fields_independently() {
        with_users_table(
            "quoted_ident_case_distinct_fulltext_search",
            |cassie, session| {
                // Arrange
                create_case_distinct_fulltext_fixture(cassie, session);

                // Act
                let matched_upper = run(
                    cassie,
                    session,
                    "SELECT id FROM fulltext_case_fields WHERE search(\"Body\", 'amber') \
                 ORDER BY id",
                );
                let top_upper = run(
                    cassie,
                    session,
                    "SELECT id, search_score(\"Body\", 'amber') AS score \
                 FROM fulltext_case_fields WHERE search(\"Body\", 'amber') \
                 ORDER BY score DESC LIMIT 1",
                );

                // Assert
                assert_eq!(
                    matched_upper.rows,
                    vec![
                        vec![Value::Int64(2)],
                        vec![Value::Int64(3)],
                        vec![Value::Int64(4)]
                    ]
                );
                assert_eq!(top_upper.rows[0][0], Value::Int64(4));
                assert!(matches!(top_upper.rows[0][1], Value::Float64(score) if score > 0.0));
            },
        );
    }

    #[test]
    fn should_preserve_case_distinct_text_statistics_through_a_join() {
        with_users_table(
            "quoted_ident_case_distinct_join_fulltext",
            |cassie, session| {
                // Arrange
                run(
                    cassie,
                    session,
                    "CREATE TABLE join_fulltext_left (id INT, \"body\" TEXT, \"Body\" TEXT)",
                );
                run(
                    cassie,
                    session,
                    "CREATE TABLE join_fulltext_right (left_id INT)",
                );
                run(
                    cassie,
                    session,
                    "INSERT INTO join_fulltext_left (id, \"body\", \"Body\") VALUES \
                 (1, 'amber amber', 'violet'), (2, 'violet', 'amber amber')",
                );
                run(
                    cassie,
                    session,
                    "INSERT INTO join_fulltext_right (left_id) VALUES (1), (2)",
                );

                // Act
                let matched = run(
                    cassie,
                    session,
                    "SELECT join_fulltext_left.id, search_score(\"Body\", 'amber') AS score \
                 FROM join_fulltext_left JOIN join_fulltext_right \
                 ON join_fulltext_left.id = join_fulltext_right.left_id \
                 WHERE search(\"Body\", 'amber') ORDER BY join_fulltext_left.id",
                );

                // Assert
                assert_eq!(matched.rows.len(), 1);
                assert_eq!(matched.rows[0][0], Value::Int64(2));
                assert!(matches!(matched.rows[0][1], Value::Float64(score) if score > 0.0));
            },
        );
    }

    #[test]
    fn should_group_case_distinct_delimited_columns_independently() {
        with_users_table("quoted_ident_group_case_distinct", |cassie, session| {
            // Arrange
            run(
                cassie,
                session,
                "CREATE TABLE grouped_case_distinct (\"a\" TEXT, \"A\" TEXT)",
            );
            run(
                cassie,
                session,
                "INSERT INTO grouped_case_distinct (\"a\", \"A\") VALUES \
                 ('north', 'east'), ('north', 'west'), ('north', 'east'), ('south', 'east')",
            );

            // Act
            let grouped = run(
                cassie,
                session,
                "SELECT \"a\", \"A\", count(*) FROM grouped_case_distinct \
                 GROUP BY \"a\", \"A\" ORDER BY \"a\", \"A\"",
            );

            // Assert
            assert_eq!(
                grouped.rows,
                vec![
                    vec![
                        Value::String("north".to_string()),
                        Value::String("east".to_string()),
                        Value::Int64(2),
                    ],
                    vec![
                        Value::String("north".to_string()),
                        Value::String("west".to_string()),
                        Value::Int64(1),
                    ],
                    vec![
                        Value::String("south".to_string()),
                        Value::String("east".to_string()),
                        Value::Int64(1),
                    ],
                ]
            );
        });
    }

    #[test]
    fn should_preserve_case_distinct_column_names_across_restart() {
        // Arrange
        use_local_storage();
        let path = data_dir("quoted_ident_case_distinct_restart");
        {
            let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
            cassie.startup().expect("start Cassie");
            let session = cassie.create_session("tester", None);
            run(
                &cassie,
                &session,
                "CREATE TABLE persisted_case_distinct (\"a\" INT, \"A\" INT)",
            );
            run(
                &cassie,
                &session,
                "INSERT INTO persisted_case_distinct (\"a\", \"A\") VALUES (11, 22)",
            );
        }

        // Act
        let restarted = Cassie::new_with_data_dir(&path).expect("reopen Cassie");
        restarted.startup().expect("restart Cassie");
        let session = restarted.create_session("tester", None);
        let lower_exact = run(
            &restarted,
            &session,
            "SELECT \"a\" FROM persisted_case_distinct",
        );
        let upper_exact = run(
            &restarted,
            &session,
            "SELECT \"A\" FROM persisted_case_distinct",
        );
        let unquoted_upper = run(
            &restarted,
            &session,
            "SELECT A FROM persisted_case_distinct",
        );

        // Assert
        assert_eq!(lower_exact.rows, vec![vec![Value::Int64(11)]]);
        assert_eq!(upper_exact.rows, vec![vec![Value::Int64(22)]]);
        assert_eq!(unquoted_upper.rows, vec![vec![Value::Int64(11)]]);

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_preserve_exact_spelling_in_copy_column_references() {
        // Arrange
        let sql = "COPY exact_case (\"Email\") FROM STDIN";

        // Act
        let parsed = parse_statement(sql).expect("parse COPY column identifier");

        // Assert
        let QueryStatement::Copy(statement) = parsed.statement else {
            panic!("expected COPY statement");
        };
        assert_eq!(statement.columns, vec!["\"Email\""]);
    }

    #[test]
    fn should_copy_into_case_distinct_columns_independently() {
        with_users_table("quoted_ident_copy_case_distinct", |cassie, session| {
            // Arrange
            run(
                cassie,
                session,
                "CREATE TABLE copy_case_distinct (\"a\" INT, \"A\" INT)",
            );
            let parsed = parse_statement(
                "COPY copy_case_distinct (\"a\", \"A\") FROM STDIN WITH (FORMAT csv)",
            )
            .expect("parse COPY statement");
            let QueryStatement::Copy(statement) = parsed.statement else {
                panic!("expected COPY statement");
            };

            // Act
            let copied = cassie
                .copy_from_csv_stdin(session, &statement, b"11,22\n")
                .expect("copy case-distinct fields");
            let selected = run(
                cassie,
                session,
                "SELECT \"a\", \"A\" FROM copy_case_distinct",
            );

            // Assert
            assert_eq!(copied, 1);
            assert_eq!(
                selected.rows,
                vec![vec![Value::Int64(11), Value::Int64(22)]]
            );
        });
    }

    #[test]
    fn should_rename_only_the_exact_delimited_column() {
        with_users_table("quoted_ident_exact_rename", |cassie, session| {
            // Arrange
            run(
                cassie,
                session,
                "CREATE TABLE rename_case (\"a\" INT, \"A\" INT)",
            );
            run(
                cassie,
                session,
                "INSERT INTO rename_case (\"a\", \"A\") VALUES (1, 2)",
            );

            // Act
            run(
                cassie,
                session,
                "ALTER TABLE rename_case RENAME COLUMN \"A\" TO \"Upper\"",
            );
            let selected = run(cassie, session, "SELECT \"a\", \"Upper\" FROM rename_case");
            let old_upper_name =
                cassie.execute_sql(session, "SELECT \"A\" FROM rename_case", vec![]);

            // Assert
            assert_eq!(selected.rows, vec![vec![Value::Int64(1), Value::Int64(2)]]);
            assert!(old_upper_name.is_err());
        });
    }

    #[test]
    fn should_resolve_correlated_case_distinct_columns_independently() {
        with_users_table("quoted_ident_correlated_exists", |cassie, session| {
            // Arrange
            run(
                cassie,
                session,
                "CREATE TABLE exact_outer (\"a\" INT, \"A\" INT)",
            );
            run(
                cassie,
                session,
                "INSERT INTO exact_outer (\"a\", \"A\") VALUES (1, 2)",
            );
            run(
                cassie,
                session,
                "CREATE TABLE exact_inner (\"a\" INT, \"A\" INT)",
            );
            run(
                cassie,
                session,
                "INSERT INTO exact_inner (\"a\", \"A\") VALUES (1, 2)",
            );

            // Act
            let mismatched = run(
                cassie,
                session,
                "SELECT exact_outer.\"a\" FROM exact_outer WHERE EXISTS \
                 (SELECT 1 FROM exact_inner WHERE exact_inner.\"A\" = exact_outer.\"a\")",
            );
            let exact = run(
                cassie,
                session,
                "SELECT exact_outer.\"a\" FROM exact_outer WHERE EXISTS \
                 (SELECT 1 FROM exact_inner WHERE exact_inner.\"A\" = exact_outer.\"A\")",
            );

            // Assert
            assert_eq!(mismatched.rows, Vec::<Vec<Value>>::new());
            assert_eq!(exact.rows, vec![vec![Value::Int64(1)]]);
        });
    }

    #[test]
    fn should_filter_case_distinct_columns_independently() {
        with_users_table("quoted_ident_case_distinct_filter", |cassie, session| {
            // Arrange
            create_case_distinct_dml_fixture(cassie, session);

            // Act
            let filtered = run(
                cassie,
                session,
                "SELECT \"a\" FROM separate_case WHERE \"A\" = 2",
            );

            // Assert
            assert_eq!(filtered.rows, vec![vec![Value::Int64(1)]]);
        });
    }

    #[test]
    fn should_update_case_distinct_columns_independently() {
        with_users_table("quoted_ident_case_distinct_update", |cassie, session| {
            // Arrange
            create_case_distinct_dml_fixture(cassie, session);

            // Act
            let updated = run(
                cassie,
                session,
                "UPDATE separate_case SET \"A\" = 3 WHERE \"a\" = 1",
            );
            let selected = run(cassie, session, "SELECT \"a\", \"A\" FROM separate_case");

            // Assert
            assert_eq!(updated.command, "UPDATE 1");
            assert_eq!(selected.rows, vec![vec![Value::Int64(1), Value::Int64(3)]]);
        });
    }

    #[test]
    fn should_enforce_unique_constraints_on_case_distinct_columns_independently() {
        with_users_table("quoted_ident_case_distinct_unique", |cassie, session| {
            // Arrange
            run(
                cassie,
                session,
                "CREATE TABLE unique_case (\"a\" INT UNIQUE, \"A\" INT UNIQUE)",
            );
            run(
                cassie,
                session,
                "INSERT INTO unique_case (\"a\", \"A\") VALUES (1, 2)",
            );

            // Act
            let duplicate_lower = cassie.execute_sql(
                session,
                "INSERT INTO unique_case (\"a\", \"A\") VALUES (1, 3)",
                vec![],
            );
            let duplicate_upper = cassie.execute_sql(
                session,
                "INSERT INTO unique_case (\"a\", \"A\") VALUES (4, 2)",
                vec![],
            );

            // Assert
            assert!(duplicate_lower.is_err());
            assert!(duplicate_upper.is_err());
        });
    }

    #[test]
    fn should_not_use_an_index_on_a_case_distinct_sibling_column() {
        with_users_table("quoted_ident_case_distinct_index", |cassie, session| {
            // Arrange
            run(
                cassie,
                session,
                "CREATE TABLE indexed_case (\"a\" INT, \"A\" INT)",
            );
            run(
                cassie,
                session,
                "INSERT INTO indexed_case (\"a\", \"A\") VALUES (1, 20)",
            );
            run(
                cassie,
                session,
                "INSERT INTO indexed_case (\"a\", \"A\") VALUES (2, 10)",
            );
            run(
                cassie,
                session,
                "CREATE INDEX indexed_case_upper_idx ON indexed_case (\"A\")",
            );

            // Act
            let sibling_plan = run(
                cassie,
                session,
                "EXPLAIN SELECT \"a\" FROM indexed_case WHERE \"a\" = 1",
            );
            let matching_plan = run(
                cassie,
                session,
                "EXPLAIN SELECT \"A\" FROM indexed_case WHERE \"A\" = 10",
            );
            let Value::String(sibling_plan) = &sibling_plan.rows[0][0] else {
                panic!("expected sibling explain string");
            };
            let Value::String(matching_plan) = &matching_plan.rows[0][0] else {
                panic!("expected matching explain string");
            };
            let selected = run(
                cassie,
                session,
                "SELECT \"a\" FROM indexed_case WHERE \"a\" = 1",
            );
            let matching = run(
                cassie,
                session,
                "SELECT \"A\" FROM indexed_case WHERE \"A\" = 10",
            );

            // Assert
            assert!(
                sibling_plan.contains("index=none"),
                "the case-distinct sibling should not use the upper-case index"
            );
            assert!(
                matching_plan.contains("index=postgres.public.indexed_case_upper_idx"),
                "the exact-case query should use the matching upper-case index"
            );
            assert_eq!(selected.rows, vec![vec![Value::Int64(1)]]);
            assert_eq!(matching.rows, vec![vec![Value::Int64(10)]]);
        });
    }
}

// Formerly tests/parser_functions_roles.rs.
mod parser_functions_roles {
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
            let parsed =
                parse_statement("SELECT snippet(body, 'q') FROM binder_docs_snippet").unwrap();
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
}

// Formerly tests/parser_indexes.rs.
mod parser_indexes {
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
    fn should_parse_create_index_statement() {
        // Arrange
        let sql = "CREATE UNIQUE INDEX idx_users_email ON users USING btree (email) WITH (fillfactor = 90, case_sensitive = 'false')";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::CreateIndex(statement) = parsed.statement else {
            panic!("expected create index statement");
        };

        assert_eq!(statement.name, "idx_users_email");
        assert_eq!(statement.table, "users");
        assert_eq!(statement.fields, vec!["email".to_string()]);
        assert!(statement.unique);
        assert!(matches!(statement.kind, cassie::catalog::IndexKind::Scalar));
        assert_eq!(statement.options.get("fillfactor"), Some(&"90".to_string()));
        assert_eq!(
            statement.options.get("case_sensitive"),
            Some(&"false".to_string())
        );
    }

    #[test]
    fn should_parse_create_index_if_not_exists_variants() {
        // Arrange
        let statements = [
            ("CREATE INDEX IF NOT EXISTS idx_a ON docs (title)", false),
            (
                "CREATE UNIQUE INDEX IF NOT EXISTS idx_b ON docs (title)",
                true,
            ),
        ];

        for (sql, unique) in statements {
            // Act
            let parsed = parse_statement(sql).expect("parse should succeed");
            let QueryStatement::CreateIndex(statement) = parsed.statement else {
                panic!("expected create index statement");
            };

            // Assert
            assert!(statement.if_not_exists);
            assert_eq!(statement.unique, unique);
        }
    }

    #[test]
    fn should_parse_composite_create_index_statement() {
        // Arrange
        let sql = "CREATE INDEX idx_docs_title_score ON docs USING btree (title, score)";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::CreateIndex(statement) = parsed.statement else {
            panic!("expected create index statement");
        };

        assert_eq!(statement.name, "idx_docs_title_score");
        assert_eq!(statement.table, "docs");
        assert_eq!(
            statement.fields,
            vec!["title".to_string(), "score".to_string()]
        );
        assert!(!statement.unique);
        assert!(matches!(statement.kind, IndexKind::Scalar));
    }

    #[test]
    fn should_parse_create_index_include_columns() {
        // Arrange
        let sql = "CREATE INDEX idx_docs_title_include ON docs USING btree (title) INCLUDE (body, score) WITH (fillfactor = 90)";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::CreateIndex(statement) = parsed.statement else {
            panic!("expected create index statement");
        };
        assert_eq!(statement.name, "idx_docs_title_include");
        assert_eq!(statement.fields, vec!["title".to_string()]);
        assert_eq!(
            statement.include_fields,
            vec!["body".to_string(), "score".to_string()]
        );
        assert_eq!(statement.options.get("fillfactor"), Some(&"90".to_string()));
    }

    #[test]
    fn should_parse_create_partial_index_statement() {
        // Arrange
        let sql =
            "CREATE INDEX idx_docs_active ON docs USING btree (title) WHERE status = 'active'";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::CreateIndex(statement) = parsed.statement else {
            panic!("expected create index statement");
        };
        assert_eq!(statement.name, "idx_docs_active");
        assert_eq!(statement.fields, vec!["title".to_string()]);
        assert!(statement.predicate.is_some());
    }

    #[test]
    fn should_parse_create_expression_index_statement() {
        // Arrange
        let sql = "CREATE INDEX idx_docs_lower_title ON docs USING btree (lower(title))";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::CreateIndex(statement) = parsed.statement else {
            panic!("expected create index statement");
        };
        assert_eq!(statement.name, "idx_docs_lower_title");
        assert_eq!(statement.table, "docs");
        assert_eq!(statement.fields, [] as [std::string::String; 0]);
        assert_eq!(statement.expressions.len(), 1);
    }

    #[test]
    fn should_reject_non_scalar_expression_index() {
        // Arrange
        let sql = "CREATE INDEX idx_docs_lower_title ON docs USING fulltext (lower(title))";

        // Act
        let err = parse_statement(sql).expect_err("parse should reject expression fulltext index");

        // Assert
        assert_eq!(err.kind(), cassie::sql::SqlErrorKind::Unsupported);
        assert!(err
            .message()
            .contains("expression indexes are only supported for scalar index methods"));
    }

    #[test]
    fn should_reject_non_immutable_function_expression_index() {
        // Arrange
        let cassie = Cassie::new_with_data_dir(format!(
            "/tmp/cassie-expression-index-volatile-{}",
            Uuid::new_v4()
        ))
        .unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        cassie.register_collection(
            "expression_index_volatile_docs".to_string(),
            Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            },
        );
        let parsed = parse_statement(
            "CREATE INDEX idx_expression_volatile ON expression_index_volatile_docs USING btree (current_user())",
        )
        .expect("parse should succeed");

        // Act
        let err = cassie::sql::binder::bind(parsed, &cassie.catalog)
            .expect_err("bind should reject volatile function");

        // Assert
        assert!(err
            .to_string()
            .contains("function 'current_user' is not immutable for index expressions"));
    });
    }

    #[test]
    fn should_reject_duplicate_include_columns() {
        // Arrange
        let cassie =
            Cassie::new_with_data_dir(format!("/tmp/cassie-include-duplicate-{}", Uuid::new_v4()))
                .unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        cassie.register_collection(
            "include_duplicate_docs".to_string(),
            Schema {
                fields: vec![
                    FieldSchema {
                        name: "title".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                    FieldSchema {
                        name: "body".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                ],
            },
        );

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_include_duplicate ON include_duplicate_docs USING btree (title) INCLUDE (body, body)",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        // Assert
        assert!(bound.is_err());
    });
    }

    #[test]
    fn should_reject_include_key_overlap() {
        // Arrange
        let cassie =
            Cassie::new_with_data_dir(format!("/tmp/cassie-include-overlap-{}", Uuid::new_v4()))
                .unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        cassie.register_collection(
            "include_overlap_docs".to_string(),
            Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            },
        );

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_include_overlap ON include_overlap_docs USING btree (title) INCLUDE (title)",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        // Assert
        assert!(bound.is_err());
    });
    }

    #[test]
    fn should_reject_unknown_include_column() {
        // Arrange
        let cassie =
            Cassie::new_with_data_dir(format!("/tmp/cassie-include-unknown-{}", Uuid::new_v4()))
                .unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        cassie.register_collection(
            "include_unknown_docs".to_string(),
            Schema {
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    data_type: DataType::Text,
                    nullable: true,
                }],
            },
        );

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_include_unknown ON include_unknown_docs USING btree (title) INCLUDE (body)",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        // Assert
        assert!(bound.is_err());
    });
    }

    #[test]
    fn should_reject_fulltext_include_columns() {
        // Arrange
        let cassie =
            Cassie::new_with_data_dir(format!("/tmp/cassie-include-fulltext-{}", Uuid::new_v4()))
                .unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        cassie.register_collection(
            "include_fulltext_docs".to_string(),
            Schema {
                fields: vec![
                    FieldSchema {
                        name: "title".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                    FieldSchema {
                        name: "body".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    },
                ],
            },
        );

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_include_fulltext ON include_fulltext_docs USING fulltext (body) INCLUDE (title)",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        // Assert
        assert!(bound.is_err());
    });
    }

    #[test]
    fn should_reject_composite_vector_create_index_statement() {
        // Arrange
        let sql = "CREATE INDEX idx_docs_embedding ON docs USING vector (embedding, source)";

        // Act
        let parsed = parse_statement(sql);

        // Assert
        assert!(parsed.is_err());
    }

    #[test]
    fn should_reject_composite_fulltext_create_index_statement() {
        // Arrange
        let sql = "CREATE INDEX idx_docs_body_title ON docs USING fulltext (body, title)";

        // Act
        let parsed = parse_statement(sql);

        // Assert
        assert!(parsed.is_err());
    }

    #[test]
    fn should_parse_vector_create_index_statement() {
        // Arrange
        let sql = "CREATE INDEX idx_docs_embedding ON docs USING vector (embedding) WITH (source_field = content, metric = 'l2')";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::CreateIndex(statement) = parsed.statement else {
            panic!("expected create index statement");
        };

        assert_eq!(statement.name, "idx_docs_embedding");
        assert_eq!(statement.table, "docs");
        assert_eq!(statement.fields, vec!["embedding".to_string()]);
        assert!(!statement.unique);
        assert!(matches!(statement.kind, IndexKind::Vector));
        assert_eq!(
            statement.options.get("source_field"),
            Some(&"content".to_string())
        );
        assert_eq!(statement.options.get("metric"), Some(&"l2".to_string()));
    }

    #[test]
    fn should_parse_fulltext_create_index_statement_with_options() {
        // Arrange
        let cassie = Cassie::new_with_data_dir(format!(
            "/tmp/cassie-fulltext-index-options-{}",
            Uuid::new_v4()
        ))
        .unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        cassie
            .register_collection(
                "ft_docs_options".to_string(),
                Schema {
                    fields: vec![
                        FieldSchema {
                            name: "id".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                        FieldSchema {
                            name: "body".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                    ],
                },
            )
            ;

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_ft_docs_body ON ft_docs_options USING fulltext (body) WITH (boost = 2.5, k1 = 0.8, b = 0.1)",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog)
            .expect("bind should succeed");

        // Assert
        let QueryStatement::CreateIndex(statement) = bound.statement.statement else {
            panic!("expected create index statement");
        };
        assert!(matches!(statement.kind, IndexKind::FullText));
        assert_eq!(statement.options.get("boost"), Some(&"2.5".to_string()));
        assert_eq!(statement.options.get("k1"), Some(&"0.8".to_string()));
        assert_eq!(statement.options.get("b"), Some(&"0.1".to_string()));
    });
    }

    #[test]
    fn should_apply_fulltext_create_index_defaults() {
        // Arrange
        let cassie = Cassie::new_with_data_dir(format!(
            "/tmp/cassie-fulltext-index-defaults-{}",
            Uuid::new_v4()
        ))
        .unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            cassie.register_collection(
                "ft_docs_defaults".to_string(),
                Schema {
                    fields: vec![
                        FieldSchema {
                            name: "id".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                        FieldSchema {
                            name: "body".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                    ],
                },
            );

            // Act
            let parsed = parse_statement(
                "CREATE INDEX idx_ft_docs_defaults ON ft_docs_defaults USING fulltext (body)",
            )
            .expect("parse should succeed");
            let bound =
                cassie::sql::binder::bind(parsed, &cassie.catalog).expect("bind should succeed");

            // Assert
            let QueryStatement::CreateIndex(statement) = bound.statement.statement else {
                panic!("expected create index statement");
            };
            assert_eq!(statement.options.get("boost"), Some(&"1".to_string()));
            assert_eq!(statement.options.get("k1"), Some(&"1.2".to_string()));
            assert_eq!(statement.options.get("b"), Some(&"0.75".to_string()));
        });
    }

    #[test]
    fn should_reject_fulltext_create_index_with_non_finite_boost() {
        // Arrange
        let cassie = Cassie::new_with_data_dir(format!(
            "/tmp/cassie-fulltext-index-non-finite-{}",
            Uuid::new_v4()
        ))
        .unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        cassie
            .register_collection(
                "ft_docs_non_finite".to_string(),
                Schema {
                    fields: vec![
                        FieldSchema {
                            name: "id".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                        FieldSchema {
                            name: "body".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                    ],
                },
            )
            ;

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_ft_docs_non_finite ON ft_docs_non_finite USING fulltext (body) WITH (boost = inf)",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        // Assert
        assert!(bound.is_err());
    });
    }

    #[test]
    fn should_reject_duplicate_fulltext_index_on_same_field() {
        // Arrange
        let cassie = Cassie::new_with_data_dir(format!(
            "/tmp/cassie-fulltext-index-duplicate-{}",
            Uuid::new_v4()
        ))
        .unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        cassie
            .register_collection(
                "ft_docs_duplicate".to_string(),
                Schema {
                    fields: vec![
                        FieldSchema {
                            name: "id".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                        FieldSchema {
                            name: "body".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                    ],
                },
            )
            ;

        cassie
            .catalog
            .register_index(IndexMeta {
                collection: "ft_docs_duplicate".to_string(),
                name: "idx_ft_docs_duplicate_primary".to_string(),
                field: "body".to_string(),
                fields: vec!["body".to_string()],
                expressions: Vec::new(),
                include_fields: Vec::new(),
                predicate: None,
                kind: IndexKind::FullText,
                unique: false,
                options: BTreeMap::from_iter(vec![
                    ("boost".to_string(), "1.0".to_string()),
                    ("k1".to_string(), "1.2".to_string()),
                    ("b".to_string(), "0.75".to_string()),
                ]),
            });

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_ft_docs_duplicate_secondary ON ft_docs_duplicate USING fulltext (body)",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        // Assert
        assert!(bound.is_err());
    });
    }

    #[test]
    fn should_reject_fulltext_index_on_non_text_field() {
        // Arrange
        let cassie = Cassie::new_with_data_dir(format!(
            "/tmp/cassie-fulltext-index-non-text-{}",
            Uuid::new_v4()
        ))
        .unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            cassie.register_collection(
                "ft_docs_bad_field".to_string(),
                Schema {
                    fields: vec![FieldSchema {
                        name: "score".to_string(),
                        data_type: DataType::Int,
                        nullable: true,
                    }],
                },
            );

            // Act
            let parsed = parse_statement(
                "CREATE INDEX idx_ft_docs_bad_field ON ft_docs_bad_field USING fulltext (score)",
            )
            .expect("parse should succeed");
            let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

            // Assert
            assert!(bound.is_err());
        });
    }

    #[test]
    fn should_reject_fulltext_create_index_with_unsupported_option() {
        // Arrange
        let cassie = Cassie::new_with_data_dir(format!(
            "/tmp/cassie-fulltext-index-unsupported-option-{}",
            Uuid::new_v4()
        ))
        .unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        cassie
            .register_collection(
                "ft_docs_unsupported".to_string(),
                Schema {
                    fields: vec![
                        FieldSchema {
                            name: "id".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                        FieldSchema {
                            name: "body".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                    ],
                },
            )
            ;

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_ft_docs_unsupported ON ft_docs_unsupported USING fulltext (body) WITH (alpha = 0.5)",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        // Assert
        assert!(bound.is_err());
    });
    }

    #[test]
    fn should_reject_fulltext_create_index_with_invalid_fulltext_k1() {
        // Arrange
        let cassie = Cassie::new_with_data_dir(format!(
            "/tmp/cassie-fulltext-index-bad-k1-{}",
            Uuid::new_v4()
        ))
        .unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        cassie
            .register_collection(
                "ft_docs_bad_k1".to_string(),
                Schema {
                    fields: vec![
                        FieldSchema {
                            name: "id".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                        FieldSchema {
                            name: "body".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                    ],
                },
            )
            ;

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_ft_docs_bad_k1 ON ft_docs_bad_k1 USING fulltext (body) WITH (k1 = -1)",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        // Assert
        assert!(bound.is_err());
    });
    }

    #[test]
    fn should_reject_vector_create_index_without_source_field() {
        // Arrange
        let cassie = Cassie::new_with_data_dir(format!(
            "/tmp/cassie-vector-index-no-source-{}",
            Uuid::new_v4()
        ))
        .unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            cassie.register_collection(
                "vec_docs_no_source".to_string(),
                Schema {
                    fields: vec![
                        FieldSchema {
                            name: "id".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                        FieldSchema {
                            name: "content".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                        FieldSchema {
                            name: "embedding".to_string(),
                            data_type: DataType::Vector(3),
                            nullable: true,
                        },
                    ],
                },
            );

            // Act
            let parsed = parse_statement(
                "CREATE INDEX idx_docs_embedding ON vec_docs_no_source USING vector (embedding)",
            )
            .expect("parse should succeed");
            let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

            // Assert
            assert!(bound.is_err());
        });
    }

    #[test]
    fn should_reject_vector_create_index_with_invalid_metric() {
        // Arrange
        let cassie = Cassie::new_with_data_dir(format!(
            "/tmp/cassie-vector-index-bad-metric-{}",
            Uuid::new_v4()
        ))
        .unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        cassie
            .register_collection(
                "vec_docs_invalid_metric".to_string(),
                Schema {
                    fields: vec![
                        FieldSchema {
                            name: "content".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                        FieldSchema {
                            name: "embedding".to_string(),
                            data_type: DataType::Vector(3),
                            nullable: true,
                        },
                    ],
                },
            )
            ;

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_docs_embedding ON vec_docs_invalid_metric USING vector (embedding) WITH (source_field = content, metric = 'unsupported')",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        // Assert
        assert!(bound.is_err());
    });
    }

    #[test]
    fn should_reject_vector_create_index_on_non_vector_field() {
        // Arrange
        let cassie = Cassie::new_with_data_dir(format!(
            "/tmp/cassie-vector-index-non-vector-{}",
            Uuid::new_v4()
        ))
        .unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        cassie
            .register_collection(
                "vec_docs_not_vector".to_string(),
                Schema {
                    fields: vec![
                        FieldSchema {
                            name: "content".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                    ],
                },
            );

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_docs_embedding ON vec_docs_not_vector USING vector (content) WITH (source_field = content)",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog);

        // Assert
        assert!(bound.is_err());
    });
    }

    #[test]
    fn should_default_vector_metric_to_cosine() {
        // Arrange
        let cassie = Cassie::new_with_data_dir(format!(
            "/tmp/cassie-vector-index-default-metric-{}",
            Uuid::new_v4()
        ))
        .unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        cassie
            .register_collection(
                "vec_docs_default_metric".to_string(),
                Schema {
                    fields: vec![
                        FieldSchema {
                            name: "content".to_string(),
                            data_type: DataType::Text,
                            nullable: true,
                        },
                        FieldSchema {
                            name: "embedding".to_string(),
                            data_type: DataType::Vector(3),
                            nullable: true,
                        },
                    ],
                },
            )
            ;

        // Act
        let parsed = parse_statement(
            "CREATE INDEX idx_docs_embedding ON vec_docs_default_metric USING vector (embedding) WITH (source_field = content)",
        )
        .expect("parse should succeed");
        let bound = cassie::sql::binder::bind(parsed, &cassie.catalog)
            .expect("bind should succeed");

        // Assert
        let QueryStatement::CreateIndex(statement) = bound.statement.statement else {
            panic!("expected create index statement");
        };
        assert_eq!(statement.options.get("metric"), Some(&"cosine".to_string()));
    });
    }

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
}

// Formerly tests/parser_sources_sets.rs.
mod parser_sources_sets {
    #![allow(unused_imports)]

    use cassie::app::Cassie;
    use cassie::catalog::{IndexKind, IndexMeta};
    use cassie::sql::ast::{
        BinaryOp, CteQuery, Expr, InsertSource, JoinKind, QuerySource, QueryStatement, SelectItem,
        SetOperator, SortDirection,
    };
    use cassie::sql::parse_statement;
    use cassie::sql::IdentifierPath;
    use cassie::types::{DataType, FieldSchema, Schema};
    use std::collections::BTreeMap;
    use uuid::Uuid;

    #[test]
    fn should_parse_inner_join_source() {
        // Arrange
        let sql = "SELECT users.name FROM users JOIN orders ON users.id = orders.user_id";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        assert!(matches!(
            statement.source,
            QuerySource::Join {
                kind: JoinKind::Inner,
                ..
            }
        ));
    }

    #[test]
    fn should_parse_chained_joins_as_left_associative_sources() {
        // Arrange
        let sql = "SELECT users.name FROM users JOIN orders ON users.id = orders.user_id JOIN regions ON orders.region_id = regions.id";

        // Act
        let parsed = parse_statement(sql).expect("join chain should parse");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        let QuerySource::Join {
            left, right, kind, ..
        } = statement.source
        else {
            panic!("expected outer join source");
        };
        assert_eq!(kind, JoinKind::Inner);
        assert!(matches!(*right, QuerySource::Collection(ref name) if name.as_str() == "regions"));
        assert!(matches!(
            *left,
            QuerySource::Join {
                kind: JoinKind::Inner,
                ..
            }
        ));
    }

    #[test]
    fn should_parse_left_join_source() {
        // Arrange
        let sql = "SELECT users.name FROM users LEFT JOIN orders ON users.id = orders.user_id";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        assert!(matches!(
            statement.source,
            QuerySource::Join {
                kind: JoinKind::Left,
                ..
            }
        ));
    }

    #[test]
    fn should_parse_right_join_source() {
        // Arrange
        let sql = "SELECT users.name FROM users RIGHT JOIN orders ON users.id = orders.user_id";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        assert!(matches!(
            statement.source,
            QuerySource::Join {
                kind: JoinKind::Right,
                ..
            }
        ));
    }

    #[test]
    fn should_parse_full_outer_join_source() {
        // Arrange
        let sql = "SELECT users.name FROM users FULL JOIN orders ON users.key = orders.user_key";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        assert!(matches!(
            statement.source,
            QuerySource::Join {
                kind: JoinKind::Full,
                ..
            }
        ));
    }

    #[test]
    fn should_parse_cross_join_source() {
        // Arrange
        let sql = "SELECT users.name FROM users CROSS JOIN orders";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        assert!(matches!(
            statement.source,
            QuerySource::Join {
                kind: JoinKind::Cross,
                ..
            }
        ));
    }

    #[test]
    fn should_reject_right_join_without_on_predicate() {
        // Arrange
        let sql = "SELECT users.name FROM users RIGHT JOIN orders";

        // Act
        let error = parse_statement(sql).expect_err("RIGHT JOIN without ON should fail");

        // Assert
        assert_eq!(error.kind(), cassie::sql::SqlErrorKind::Syntax);
        assert!(error.message().contains("JOIN requires ON"));
    }

    #[test]
    fn should_reject_cross_join_with_on_predicate() {
        // Arrange
        let sql = "SELECT users.name FROM users CROSS JOIN orders ON users.id = orders.user_id";

        // Act
        let error = parse_statement(sql).expect_err("CROSS JOIN with ON should fail");

        // Assert
        assert_eq!(error.kind(), cassie::sql::SqlErrorKind::Unsupported);
        assert!(error.message().contains("unsupported FROM syntax"));
    }

    #[test]
    fn should_parse_from_subquery_source() {
        // Arrange
        let sql = "SELECT recent.title FROM (SELECT title FROM docs) AS recent";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        assert!(matches!(
            statement.source,
            QuerySource::Subquery { ref alias, .. } if alias == "recent"
        ));
    }

    #[test]
    fn should_parse_lateral_join_source() {
        // Arrange
        let sql = "SELECT users.name FROM users JOIN LATERAL (SELECT user_key FROM orders) AS recent ON users.key = recent.user_key";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        assert!(matches!(
            statement.source,
            QuerySource::Join { ref right, .. } if matches!(right.as_ref(), QuerySource::Subquery { ref alias, lateral: true, .. } if alias == "recent")
        ));
    }

    #[test]
    fn should_parse_cross_apply_join_source() {
        // Arrange
        let sql = "SELECT users.name FROM users CROSS APPLY (SELECT total FROM orders) AS recent";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        assert!(matches!(
            statement.source,
            QuerySource::Join { kind: JoinKind::Cross, ref right, .. }
                if matches!(right.as_ref(), QuerySource::Subquery { lateral: true, .. })
        ));
    }

    #[test]
    fn should_parse_outer_apply_join_source() {
        // Arrange
        let sql = "SELECT users.name FROM users OUTER APPLY (SELECT total FROM orders) AS recent";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        assert!(matches!(
            statement.source,
            QuerySource::Join { kind: JoinKind::Left, ref right, .. }
                if matches!(right.as_ref(), QuerySource::Subquery { lateral: true, .. })
        ));
    }

    #[test]
    fn should_parse_distinct_select() {
        // Arrange
        let sql = "SELECT DISTINCT category FROM docs";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        assert!(statement.distinct);
    }

    #[test]
    fn should_parse_distinct_on_select() {
        // Arrange
        let sql =
        "SELECT DISTINCT ON (tenant_id) tenant_id, title FROM docs ORDER BY tenant_id, score DESC";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        assert!(!statement.distinct);
        assert_eq!(statement.distinct_on.len(), 1);
        assert!(matches!(&statement.distinct_on[0], Expr::Column(name) if name == "tenant_id"));
    }

    #[test]
    fn should_parse_group_by_with_having() {
        // Arrange
        let sql =
            "SELECT category, COUNT(*) AS total FROM docs GROUP BY category HAVING COUNT(*) > 1";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        assert_eq!(statement.group_by.len(), 1);
        assert!(statement.having.is_some());
    }

    #[test]
    fn should_parse_union_all_select() {
        // Arrange
        let sql = "SELECT title FROM left_docs UNION ALL SELECT title FROM right_docs";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        let set = statement.set.expect("set clause should exist");
        assert!(matches!(set.operator, SetOperator::UnionAll));
    }

    #[test]
    fn should_parse_union_all_separated_by_any_whitespace() {
        // Arrange
        let statements = [
            "SELECT title FROM left_docs UNION  ALL SELECT title FROM right_docs",
            "SELECT title FROM left_docs\n  UNION\n  ALL\n  SELECT title FROM right_docs",
            "SELECT title FROM left_docs union\tall SELECT title FROM right_docs",
        ];

        // Act
        let operators = statements.map(|sql| {
            let QueryStatement::Select(statement) = parse_statement(sql)
                .expect("parse should succeed")
                .statement
            else {
                panic!("expected select statement");
            };
            let set = statement.set.expect("set clause should exist");
            assert_eq!(
                set.right.source,
                QuerySource::Collection(
                    IdentifierPath::parse("right_docs").expect("relation path"),
                )
            );
            set.operator
        });

        // Assert
        for operator in operators {
            assert!(matches!(operator, SetOperator::UnionAll));
        }
    }

    #[test]
    fn should_parse_recursive_cte_with_union_all_on_separate_lines() {
        // Arrange
        let sql = "WITH RECURSIVE counter(n) AS (SELECT n FROM docs\nUNION\nALL\nSELECT n FROM counter) SELECT n FROM counter";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        assert!(matches!(
            statement.ctes[0].query,
            CteQuery::Recursive {
                operator: SetOperator::UnionAll,
                ..
            }
        ));
    }

    #[test]
    fn should_parse_union_select() {
        // Arrange
        let sql = "SELECT title FROM left_docs UNION SELECT title FROM right_docs";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        let set = statement.set.expect("expected set operation");
        assert!(matches!(set.operator, SetOperator::Union));
    }

    #[test]
    fn should_parse_intersect_select() {
        // Arrange
        let sql = "SELECT title FROM left_docs INTERSECT SELECT title FROM right_docs";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        let set = statement.set.expect("set clause should exist");
        assert!(matches!(set.operator, SetOperator::Intersect));
    }

    #[test]
    fn should_parse_except_select() {
        // Arrange
        let sql = "SELECT title FROM left_docs EXCEPT SELECT title FROM right_docs";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        let set = statement.set.expect("set clause should exist");
        assert!(matches!(set.operator, SetOperator::Except));
    }

    #[test]
    fn should_parse_global_order_limit_after_set_operation() {
        // Arrange
        let sql = "SELECT title FROM left_docs UNION ALL SELECT title FROM right_docs ORDER BY title LIMIT 1 OFFSET 1";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        let set = statement.set.expect("set clause should exist");
        assert!(matches!(set.operator, SetOperator::UnionAll));
        assert!(set.right.order.is_empty());
        assert_eq!(statement.order.len(), 1);
        assert_eq!(statement.limit, Some(1));
        assert_eq!(statement.offset, Some(1));
    }

    #[test]
    fn should_parse_chained_set_operation() {
        // Arrange
        let sql = "SELECT title FROM first_docs UNION ALL SELECT title FROM second_docs UNION ALL SELECT title FROM third_docs";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        let set = statement.set.expect("set operation expected");
        assert!(set.right.set.is_some());
    }

    #[test]
    fn should_parse_row_number_window_function_query() {
        // Arrange
        let sql =
        "SELECT row_number() OVER (PARTITION BY category ORDER BY score DESC) AS rank FROM docs";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        assert!(matches!(
            statement.projection.as_slice(),
            [SelectItem::WindowFunction { alias: Some(alias), .. }] if alias == "rank"
        ));
    }

    #[test]
    fn should_parse_value_window_function_query() {
        // Arrange
        let sql = "SELECT lag(title) OVER (PARTITION BY category ORDER BY score DESC) AS previous_title FROM docs";

        // Act
        let parsed = parse_statement(sql).expect("parse should succeed");

        // Assert
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected select statement");
        };
        assert!(matches!(
            statement.projection.as_slice(),
            [SelectItem::WindowFunction { alias: Some(alias), .. }] if alias == "previous_title"
        ));
    }

    #[test]
    fn should_reject_unsupported_grouping_sets_query() {
        // Arrange
        let sql = "SELECT category, COUNT(*) FROM docs GROUP BY GROUPING SETS (category)";

        // Act
        let error = parse_statement(sql).expect_err("GROUPING SETS should fail");

        // Assert
        assert_eq!(error.kind(), cassie::sql::SqlErrorKind::Unsupported);
        assert!(error.message().contains("GROUP BY"));
    }
}

// Formerly tests/parser_transport_boundaries.rs.
mod parser_transport_boundaries {
    use std::sync::Arc;
    use std::time::Duration;

    use cassie::app::{Cassie, CassieError};
    use reqwest::StatusCode;
    use tokio::io::AsyncWriteExt;
    use tokio::sync::Notify;

    use super::support_pgwire as support;

    #[test]
    fn should_apply_the_same_parser_complexity_limit_given_rest_pgwire_and_direct_sql() {
        // Arrange
        support::use_local_storage();
        let path = support::data_dir("parser-transport-limit");
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let direct_session = cassie.create_session("direct", None);
        let oversized = "x".repeat(1024 * 1024 + 1);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        // Act
        let direct = cassie.execute_sql(&direct_session, &oversized, vec![]);
        let (pgwire_fields, rest_status) = runtime.block_on(async {
            let pgwire = support::spawn_server(cassie.clone()).await;
            let mut socket = tokio::net::TcpStream::connect(pgwire.addr)
                .await
                .expect("connect pgwire");
            let (read_half, mut write_half) = socket.split();
            let mut reader = tokio::io::BufReader::new(read_half);
            support::complete_startup(&mut reader, &mut write_half).await;
            write_half
                .write_all(&support::simple_query_frame(&oversized))
                .await
                .expect("write pgwire query");
            write_half.flush().await.expect("flush pgwire query");
            let frames = support::read_frames_until_ready(&mut reader).await;
            let pgwire_fields = support::parse_error_fields(
                &frames
                    .iter()
                    .find(|frame| frame.0 == b'E')
                    .expect("pgwire resource error")
                    .1,
            );

            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind REST");
            let rest_addr = listener.local_addr().expect("REST address");
            drop(listener);
            let shutdown = Arc::new(Notify::new());
            let rest = tokio::spawn(cassie::rest::router::run_with_shutdown(
                rest_addr.to_string(),
                cassie,
                Arc::clone(&shutdown),
            ));
            tokio::time::sleep(Duration::from_millis(50)).await;
            let client = reqwest::Client::new();
            let login = client
                .post(format!("http://{rest_addr}/api/v1/auth/login"))
                .json(&serde_json::json!({
                    "username": "root",
                    "password": "postgres"
                }))
                .send()
                .await
                .expect("REST login");
            let cookie = login
                .headers()
                .get("set-cookie")
                .expect("session cookie")
                .to_str()
                .expect("cookie text")
                .split(';')
                .next()
                .expect("cookie pair")
                .to_string();
            let rest_status = client
                .post(format!("http://{rest_addr}/api/v1/admin/query-executions"))
                .header("cookie", cookie)
                .json(&serde_json::json!({
                    "database": "postgres",
                    "sql": oversized
                }))
                .send()
                .await
                .expect("REST query")
                .status();
            shutdown.notify_waiters();
            let _ = rest.await;
            pgwire.stop().await;
            (pgwire_fields, rest_status)
        });

        // Assert
        assert!(matches!(direct, Err(CassieError::ResourceLimit(_))));
        assert!(pgwire_fields
            .iter()
            .any(|(kind, value)| *kind == 'C' && value == "54000"));
        assert_eq!(rest_status, StatusCode::BAD_REQUEST);
        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/types.rs.
mod types {
    use cassie::app::Cassie;
    use cassie::types::{DataType, Value, Vector};
    use uuid::Uuid;

    fn use_local_storage() {
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
    }

    fn data_dir(label: &str) -> String {
        crate::support_temp_dirs::sweep_stale_once();
        let mut path = std::env::temp_dir();
        path.push(format!("cassie-types-{label}"));
        path.push(Uuid::new_v4().to_string());
        path.to_string_lossy().to_string()
    }

    #[test]
    fn should_parse_supported_scalar_sql_types() {
        // Arrange
        // Act
        let int = DataType::parse_sql("INT").unwrap();
        let smallint = DataType::parse_sql("SMALLINT").unwrap();
        let bigint = DataType::parse_sql("BIGINT").unwrap();
        let vector = DataType::parse_sql("vector(2)").unwrap();
        let json = DataType::parse_sql("json").unwrap();
        let jsonb = DataType::parse_sql("jsonb").unwrap();
        let uuid = DataType::parse_sql("uuid").unwrap();
        let timestamp_precision = DataType::parse_sql("timestamp(3)").unwrap();
        let char = DataType::parse_sql("char(3)").unwrap();
        let varchar = DataType::parse_sql("varchar(12)").unwrap();
        let bytea = DataType::parse_sql("bytea").unwrap();

        // Assert
        assert_eq!(int, DataType::Int);
        assert_eq!(smallint, DataType::SmallInt);
        assert_eq!(bigint, DataType::BigInt);
        assert_eq!(vector, DataType::Vector(2));
        assert_eq!(json, DataType::Json);
        assert_eq!(jsonb, DataType::Json);
        assert_eq!(uuid, DataType::Uuid);
        assert_eq!(timestamp_precision, DataType::Timestamp);
        assert_eq!(char, DataType::Char { length: Some(3) });
        assert_eq!(varchar, DataType::Varchar { length: Some(12) });
        assert_eq!(bytea, DataType::Bytea);
    }

    #[test]
    fn should_parse_supported_array_sql_types() {
        // Arrange
        // Act
        let array = DataType::parse_sql("text[]").unwrap();
        let unsupported = DataType::parse_sql("int[][]");

        // Assert
        assert_eq!(array, DataType::Array(Box::new(DataType::Text)));
        assert!(unsupported.is_err());
    }

    #[test]
    fn should_validate_deterministic_type_oid_assignments() {
        // Arrange
        let vector_two = DataType::Vector(2);
        let vector_three = DataType::Vector(3);
        let int_array = DataType::Array(Box::new(DataType::Int));

        // Act
        let vector_two_oid = vector_two.type_oid();
        let vector_three_oid = vector_three.type_oid();
        let int_array_oid = int_array.type_oid();

        // Assert
        assert_eq!(vector_two_oid, vector_three_oid - 1);
        assert_eq!(int_array_oid, 34000 + DataType::Int.type_oid() % 10000);
    }

    #[test]
    fn should_roundtrip_supported_sql_values() {
        // Arrange
        use_local_storage();
        let path = data_dir("roundtrip");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE type_round_trip (item_id TEXT, item_uuid UUID, created_on DATE, created_at TIME, created_at_ts TIMESTAMP, payload JSON, values INT[], embedding VECTOR(2), short SMALLINT, wide BIGINT, code CHAR(4), title VARCHAR(10), blob BYTEA)",
                vec![],
            )

.unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO type_round_trip (item_id, item_uuid, created_on, created_at, created_at_ts, payload, values, embedding, short, wide, code, title, blob) VALUES ('row-1', $1, $2, $3, $4, $5, $6, $7, $8, $9, 'ABCD', 'sample', '\\x01020a')",
                vec![
                    Value::String("550e8400-e29b-41d4-a716-446655440000".to_string()),
                    Value::String("2026-06-18".to_string()),
                    Value::String("12:34:56".to_string()),
                    Value::String("2026-06-18T12:34:56Z".to_string()),
                    Value::Json(serde_json::json!({"source": "types", "value": 1})),
                    Value::Json(serde_json::json!([1, 2, 3])),
                    Value::Json(serde_json::json!([0.25, 0.75])),
                    Value::Int64(12),
                    Value::Int64(9_223_372_036_854_775_807),
                ],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT item_id, item_uuid, created_on, created_at, created_at_ts, payload, values, embedding, short, wide, code, title, blob FROM type_round_trip WHERE item_id = 'row-1'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(selected.rows.len(), 1);
        assert_eq!(selected.rows[0][0], Value::String("row-1".to_string()));
        assert_eq!(
            selected.rows[0][1],
            Value::String("550e8400-e29b-41d4-a716-446655440000".to_string())
        );
        assert_eq!(selected.rows[0][2], Value::String("2026-06-18".to_string()));
        assert_eq!(selected.rows[0][3], Value::String("12:34:56".to_string()));
        assert_eq!(
            selected.rows[0][4],
            Value::String("2026-06-18T12:34:56.000000Z".to_string())
        );
        assert_eq!(
            selected.rows[0][5],
            Value::Json(serde_json::json!({"source": "types", "value": 1}))
        );
        assert_eq!(
            selected.rows[0][6],
            Value::Json(serde_json::json!([1, 2, 3]))
        );
        assert_eq!(
            selected.rows[0][7],
            Value::Vector(Vector::new(vec![0.25, 0.75]))
        );
        assert_eq!(selected.rows[0][8], Value::Int64(12));
        assert_eq!(
            selected.rows[0][9],
            Value::Int64(9_223_372_036_854_775_807)
        );
        assert_eq!(selected.rows[0][10], Value::String("ABCD".to_string()));
        assert_eq!(selected.rows[0][11], Value::String("sample".to_string()));
        assert_eq!(selected.rows[0][12], Value::String("\\x01020a".to_string()));

        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_cast_string_to_uuid() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let path = data_dir("cast_uuid");
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);

            // Act
            let casted_uuid = cassie
                .execute_sql(
                    &session,
                    "SELECT CAST('550e8400-e29b-41d4-a716-446655440000' AS UUID)",
                    vec![],
                )
                .unwrap();
            // Assert
            assert_eq!(
                casted_uuid.rows[0][0],
                Value::String("550e8400-e29b-41d4-a716-446655440000".to_string())
            );

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_cast_null_to_text() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let path = data_dir("cast_null_text");
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);

            // Act
            let casted_text = cassie
                .execute_sql(&session, "SELECT CAST(NULL AS TEXT)", vec![])
                .unwrap();

            // Assert
            assert_eq!(casted_text.rows[0][0], Value::Null);

            let _ = std::fs::remove_dir_all(path);
        });
    }

    #[test]
    fn should_fail_when_casting_scalar_to_unsupported_type_family() {
        // Arrange
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let path = data_dir("cast_unsupported_family");
            let cassie = Cassie::new_with_data_dir(&path).unwrap();
            cassie.startup().unwrap();
            let session = cassie.create_session("tester", None);

            // Act
            let vector_cast = cassie.execute_sql(&session, "SELECT CAST(1 AS VECTOR(2))", vec![]);
            let array_cast = cassie.execute_sql(&session, "SELECT CAST(1 AS INT[])", vec![]);

            // Assert
            assert!(vector_cast.is_err(), "vector cast should be unsupported");
            assert!(array_cast.is_err(), "array cast should be unsupported");
            if let Err(error) = vector_cast {
                assert!(
                    error
                        .to_string()
                        .contains("cannot cast scalar value to VECTOR"),
                    "unexpected vector cast error: {error}"
                );
            }
            if let Err(error) = array_cast {
                assert!(
                    error
                        .to_string()
                        .contains("cannot cast scalar value to ARRAY"),
                    "unexpected array cast error: {error}"
                );
            }

            let _ = std::fs::remove_dir_all(path);
        });
    }
}

// Formerly tests/sql_string_literals.rs.
mod sql_string_literals {
    use super::support_sql as support;

    use cassie::app::Cassie;
    use cassie::sql::ast::{Expr, QueryStatement, SelectItem};
    use cassie::sql::{parse_statement, SqlErrorKind};
    use cassie::types::Value;

    use support::{data_dir, use_local_storage};

    fn parse_projected_string(sql: &str) -> String {
        let parsed = parse_statement(sql).expect("parse string literal projection");
        let QueryStatement::Select(statement) = parsed.statement else {
            panic!("expected SELECT statement");
        };
        let SelectItem::Expr {
            expr: Expr::StringLiteral(value),
            ..
        } = &statement.projection[0]
        else {
            panic!("expected projected string literal");
        };
        value.clone()
    }

    #[test]
    fn should_unescape_every_doubled_quote_in_sql_string_literals() {
        // Arrange
        let cases = [
            ("SELECT 'O''Brien'", "O'Brien"),
            ("SELECT ''''", "'"),
            ("SELECT 'one''two''three'", "one'two'three"),
        ];

        // Act
        let parsed = cases.map(|(sql, expected)| (parse_projected_string(sql), expected));

        // Assert
        for (actual, expected) in parsed {
            assert_eq!(actual, expected);
        }
    }

    fn statements_not_rejected_as_unsupported<'a>(statements: &[&'a str]) -> Vec<&'a str> {
        statements
            .iter()
            .copied()
            .filter(|sql| {
                !parse_statement(sql)
                    .err()
                    .is_some_and(|error| error.kind() == SqlErrorKind::Unsupported)
            })
            .collect()
    }

    #[test]
    fn should_reject_concatenation_instead_of_parsing_one_string_literal() {
        // Arrange
        let statements = [
            "SELECT 'a' || 'b'",
            "SELECT 'a'||'b'",
            "SELECT '(' || name || ')' AS y FROM n",
            "SELECT '[' || trim('  x  ') || ']' AS t",
            "SELECT name || 'x' AS y FROM n",
            "SELECT plain FROM k WHERE 'x' = 'x' || 'y'",
            "SELECT id FROM p WHERE name = 'al' || 'pha'",
            "INSERT INTO p (id, name) VALUES (2, 'a' || 'b')",
            "DELETE FROM n WHERE 'x' = 'x' || 'y'",
            "UPDATE n SET name = 'a' || 'b'",
        ];

        // Act
        let accepted = statements_not_rejected_as_unsupported(&statements);

        // Assert
        assert!(accepted.is_empty(), "not rejected: {accepted:?}");
    }

    #[test]
    fn should_reject_like_escape_instead_of_parsing_one_string_literal() {
        // Arrange
        let statements = [
            "SELECT 'abc' LIKE 'a%' ESCAPE '!' AS v",
            "SELECT name FROM n WHERE name LIKE 'a!%' ESCAPE '!'",
        ];

        // Act
        let accepted = statements_not_rejected_as_unsupported(&statements);

        // Assert
        assert!(accepted.is_empty(), "not rejected: {accepted:?}");
    }

    #[test]
    fn should_reject_adjacent_string_literals_as_one_token() {
        // Arrange
        let sql = "SELECT 'a' 'b'";

        // Act
        let parsed = parse_statement(sql);

        // Assert
        assert!(parsed.is_err(), "adjacent literals must not collapse");
    }

    #[test]
    fn should_keep_operator_text_inside_a_single_string_literal() {
        // Arrange
        let cases = [
            ("SELECT 'a || b'", "a || b"),
            ("SELECT 'a'' || ''b'", "a' || 'b"),
            ("SELECT 'x LIKE y ESCAPE z'", "x LIKE y ESCAPE z"),
            ("SELECT ''", ""),
        ];

        // Act
        let parsed = cases.map(|(sql, expected)| (parse_projected_string(sql), expected));

        // Assert
        for (actual, expected) in parsed {
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn should_round_trip_doubled_quotes_through_sql() {
        // Arrange
        use_local_storage();
        let path = data_dir("sql_string_literal_quotes");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
            cassie.startup().expect("start Cassie");
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    "CREATE TABLE sql_string_literal_quotes (position BIGINT, value TEXT)",
                    vec![],
                )
                .expect("create table");

            // Act
            cassie
                .execute_sql(
                    &session,
                    "INSERT INTO sql_string_literal_quotes (position, value) VALUES (1, 'O''Brien'), (2, ''''), (3, 'one''two''three')",
                    vec![],
                )
                .expect("insert escaped literals");
            let selected = cassie
                .execute_sql(
                    &session,
                    "SELECT value FROM sql_string_literal_quotes ORDER BY position",
                    vec![],
                )
                .expect("select escaped literals");

            // Assert
            assert_eq!(
                selected.rows,
                vec![
                    vec![Value::String("O'Brien".to_string())],
                    vec![Value::String("'".to_string())],
                    vec![Value::String("one'two'three".to_string())],
                ]
            );
            let _ = std::fs::remove_dir_all(path);
        });
    }
}

mod ddl_check_literals {
    use cassie::app::{Cassie, CassieSession};
    use cassie::types::Value;

    use super::support_sql as support;

    pub(super) fn open(label: &str, statements: &[&str]) -> (Cassie, CassieSession, String) {
        support::use_local_storage();
        let path = support::data_dir(label);
        let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
        cassie.startup().expect("start Cassie");
        let session = cassie.create_session("tester", None);
        for sql in statements {
            cassie.execute_sql(&session, sql, vec![]).expect(sql);
        }
        (cassie, session, path)
    }

    pub(super) fn rows(cassie: &Cassie, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
        cassie.execute_sql(session, sql, vec![]).expect(sql).rows
    }

    #[test]
    fn should_decode_doubled_quotes_in_check_literals() {
        // Arrange
        let (cassie, session, path) = open(
            "check_doubled_quote_literal",
            &[
                "CREATE TABLE check_quote_eq (id INT NOT NULL, b TEXT CHECK (b = 'it''s'))",
                "CREATE TABLE check_quote_ne (id INT NOT NULL, b TEXT, CONSTRAINT not_its CHECK (b <> 'it''s'))",
            ],
        );

        // Act
        let only_legal_value = cassie.execute_sql(
            &session,
            "INSERT INTO check_quote_eq (id, b) VALUES (1, 'it''s')",
            vec![],
        );
        let forbidden_value = cassie.execute_sql(
            &session,
            "INSERT INTO check_quote_ne (id, b) VALUES (1, 'it''s')",
            vec![],
        );
        let two_quote_value = cassie.execute_sql(
            &session,
            "INSERT INTO check_quote_eq (id, b) VALUES (2, 'it''''s')",
            vec![],
        );

        // Assert
        assert!(only_legal_value.is_ok(), "the CHECK value itself must pass");
        assert!(
            forbidden_value.is_err(),
            "the forbidden value must be rejected"
        );
        assert!(
            two_quote_value.is_err(),
            "a value with two quotes differs from the CHECK value"
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_store_doubled_quote_default_as_one_quote() {
        // Arrange
        let (cassie, session, path) = open(
            "default_doubled_quote_literal",
            &[
                "CREATE TABLE default_quote (id INT NOT NULL, a TEXT DEFAULT 'it''s')",
                "INSERT INTO default_quote (id) VALUES (1)",
            ],
        );

        // Act
        let stored = rows(
            &cassie,
            &session,
            "SELECT a FROM default_quote WHERE a = 'it''s'",
        );
        let column_default = rows(
            &cassie,
            &session,
            "SELECT column_default FROM information_schema.columns WHERE table_name = 'default_quote' AND column_name = 'a'",
        );

        // Assert
        assert_eq!(stored, vec![vec![Value::String("it's".to_string())]]);
        assert_eq!(
            column_default,
            vec![vec![Value::String("'it''s'".to_string())]]
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_check_comparing_against_a_non_constant() {
        // Arrange
        let (cassie, session, path) = open(
            "check_non_constant_operand",
            &["CREATE TABLE check_alter_target (lo INT, hi INT)"],
        );

        // Act
        let column_reference = cassie.execute_sql(
            &session,
            "CREATE TABLE check_columns (id INT NOT NULL, lo INT, hi INT, CHECK (lo < hi))",
            vec![],
        );
        let quoted_column = cassie.execute_sql(
            &session,
            "CREATE TABLE check_quoted_column (lo INT, hi INT CHECK (hi > \"lo\"))",
            vec![],
        );
        let concatenation = cassie.execute_sql(
            &session,
            "CREATE TABLE check_concat (b TEXT CHECK (b = 'a' || 'b'))",
            vec![],
        );
        let altered = cassie.execute_sql(
            &session,
            "ALTER TABLE check_alter_target ADD CONSTRAINT lo_below_hi CHECK (lo < hi)",
            vec![],
        );

        // Assert
        assert!(
            column_reference.is_err(),
            "a CHECK whose right side is a column must not be stored as a string"
        );
        assert!(quoted_column.is_err(), "a quoted identifier is a column");
        assert!(concatenation.is_err(), "an expression is not a constant");
        assert!(altered.is_err(), "ADD CONSTRAINT must reject it too");
        assert!(!cassie.catalog.exists("check_columns"));
        assert!(!cassie.catalog.exists("check_quoted_column"));
        assert!(!cassie.catalog.exists("check_concat"));
        let _ = std::fs::remove_dir_all(path);
    }
}

mod ddl_default_values {
    use cassie::types::Value;

    use super::ddl_check_literals::{open, rows};

    #[test]
    fn should_evaluate_volatile_function_defaults_per_row() {
        // Arrange
        let (cassie, session, path) = open(
            "default_volatile_functions",
            &[
                "CREATE TABLE default_functions (id INT NOT NULL, created_at TIMESTAMP DEFAULT now(), updated_at TIMESTAMP(3) NOT NULL DEFAULT CURRENT_TIMESTAMP, day DATE DEFAULT CURRENT_DATE, uid UUID DEFAULT gen_random_uuid())",
                "INSERT INTO default_functions (id) VALUES (1)",
                "INSERT INTO default_functions (id) VALUES (2)",
            ],
        );

        // Act
        let stored = rows(
            &cassie,
            &session,
            "SELECT created_at, updated_at, day, uid FROM default_functions ORDER BY id",
        );
        let past_rows = rows(
            &cassie,
            &session,
            "SELECT id FROM default_functions WHERE created_at > '2020-01-01T00:00:00Z' AND day > '2020-01-01' ORDER BY id",
        );

        // Assert
        assert_eq!(stored.len(), 2);
        for row in &stored {
            assert!(
                row.iter().all(|value| !matches!(value, Value::Null)),
                "every function default must produce a value"
            );
            assert!(
                !row.iter().any(|value| matches!(
                    value,
                    Value::String(text) if text.contains("()") || text.contains("CURRENT_")
                )),
                "a function default must not be stored as its own source text"
            );
        }
        assert_ne!(stored[0][3], stored[1][3], "gen_random_uuid() runs per row");
        assert_eq!(
            past_rows,
            vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_default_expressions_the_engine_cannot_evaluate() {
        // Arrange
        let (cassie, session, path) = open(
            "default_unsupported_expressions",
            &["CREATE TABLE default_alter_target (a INT, n INT)"],
        );

        // Act
        let unsupported_function = cassie.execute_sql(
            &session,
            "CREATE TABLE default_abs (a INT, n INT DEFAULT abs(-3))",
            vec![],
        );
        let wrong_type = cassie.execute_sql(
            &session,
            "CREATE TABLE default_wrong_type (a INT, n INT DEFAULT now())",
            vec![],
        );
        let text_function = cassie.execute_sql(
            &session,
            "CREATE TABLE default_text_function (a INT, label TEXT DEFAULT upper('x'))",
            vec![],
        );
        let set_default = cassie.execute_sql(
            &session,
            "ALTER TABLE default_alter_target ALTER COLUMN n SET DEFAULT abs(-3)",
            vec![],
        );
        let add_column = cassie.execute_sql(
            &session,
            "ALTER TABLE default_alter_target ADD COLUMN m INT DEFAULT now()",
            vec![],
        );
        cassie
            .execute_sql(
                &session,
                "INSERT INTO default_alter_target (a) VALUES (1)",
                vec![],
            )
            .expect("table stays insertable");

        // Assert
        assert!(unsupported_function.is_err(), "abs(-3) must be rejected");
        assert!(wrong_type.is_err(), "now() cannot default an INT column");
        assert!(text_function.is_err(), "upper('x') must be rejected");
        assert!(set_default.is_err(), "SET DEFAULT abs(-3) must be rejected");
        assert!(add_column.is_err(), "ADD COLUMN ... DEFAULT now() on INT");
        assert!(!cassie.catalog.exists("default_abs"));
        assert!(!cassie.catalog.exists("default_wrong_type"));
        assert!(!cassie.catalog.exists("default_text_function"));
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_accept_wrapped_default_constants() {
        // Arrange
        let (cassie, session, path) = open(
            "default_cast_constants",
            &[
                "CREATE TABLE default_casts (id INT NOT NULL, n INT DEFAULT (0), s VARCHAR(10) DEFAULT 'draft'::varchar, doc JSONB DEFAULT '{}'::jsonb)",
                "INSERT INTO default_casts (id) VALUES (1)",
                "INSERT INTO default_casts (id, n, s, doc) VALUES (2, 0, 'draft', '{}')",
            ],
        );

        // Act
        let stored = rows(
            &cassie,
            &session,
            "SELECT n, s, doc FROM default_casts ORDER BY id",
        );

        // Assert
        assert_eq!(stored.len(), 2);
        assert_eq!(
            stored[0], stored[1],
            "defaults must equal the explicit constants"
        );
        assert_eq!(stored[0][1], Value::String("draft".to_string()));
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_coerce_quoted_boolean_defaults_to_boolean() {
        // Arrange
        let (cassie, session, path) = open(
            "default_boolean_literals",
            &[
                "CREATE TABLE default_flags (id INT NOT NULL, a BOOLEAN DEFAULT 'true', b BOOLEAN NOT NULL DEFAULT 'f', c BOOLEAN DEFAULT true)",
                "ALTER TABLE default_flags ALTER COLUMN c SET DEFAULT 'no'",
                "INSERT INTO default_flags (id) VALUES (1)",
            ],
        );

        // Act
        let stored = rows(&cassie, &session, "SELECT a, b, c FROM default_flags");
        let numeric_default = cassie.execute_sql(
            &session,
            "CREATE TABLE default_flag_number (id INT NOT NULL, flag BOOLEAN DEFAULT 1)",
            vec![],
        );
        let invalid_spelling = cassie.execute_sql(
            &session,
            "CREATE TABLE default_flag_word (id INT NOT NULL, flag BOOLEAN DEFAULT 'maybe')",
            vec![],
        );

        // Assert
        assert_eq!(
            stored,
            vec![vec![
                Value::Bool(true),
                Value::Bool(false),
                Value::Bool(false)
            ]]
        );
        assert!(
            numeric_default.is_err(),
            "an integer default for a boolean column must be rejected"
        );
        assert!(
            invalid_spelling.is_err(),
            "'maybe' is not a boolean literal"
        );
        let _ = std::fs::remove_dir_all(path);
    }
}

// Bound string parameters compared against typed columns.
mod typed_parameter_canonicalization {
    use cassie::app::Cassie;
    use cassie::types::Value;

    fn string(value: &str) -> Value {
        Value::String(value.to_string())
    }

    fn ids(cassie: &Cassie, sql: &str, param: &str) -> Vec<Vec<Value>> {
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, sql, vec![string(param)])
            .expect("parameterized query")
            .rows
    }

    #[test]
    fn should_match_bound_string_parameters_like_inline_typed_literals() {
        // Arrange
        std::env::set_var("CASSIE_STORAGE_MODE", "memory");
        let cassie = Cassie::new_with_data_dir("unused").expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE typed_params (id TEXT, v UUID, b BYTEA, ts TIMESTAMP, iv UUID)",
            "CREATE INDEX typed_params_iv ON typed_params (iv)",
            "INSERT INTO typed_params (id, v, b, ts, iv) VALUES ('a', 'a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11', '\\xdeadbeef', '2024-01-01 12:00:00', 'a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11')",
        ] {
            cassie
                .execute_sql(&session, sql, Vec::new())
                .expect("seed typed parameter table");
        }
        let upper_uuid = "A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11";

        // Act
        let by_uuid = ids(
            &cassie,
            "SELECT id FROM typed_params WHERE v = $1",
            upper_uuid,
        );
        let by_indexed_uuid = ids(
            &cassie,
            "SELECT id FROM typed_params WHERE iv = $1",
            upper_uuid,
        );
        let by_bytea = ids(
            &cassie,
            "SELECT id FROM typed_params WHERE b = $1",
            "\\xDEADBEEF",
        );
        let by_reversed_uuid = ids(
            &cassie,
            "SELECT id FROM typed_params WHERE $1 = v",
            upper_uuid,
        );
        let by_uuid_list = ids(
            &cassie,
            "SELECT id FROM typed_params WHERE v IN ($1, 'b0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11')",
            upper_uuid,
        );
        let by_timestamp = ids(
            &cassie,
            "SELECT id FROM typed_params WHERE ts = $1",
            "2024-01-01 12:00:00",
        );

        // Assert
        let expected = vec![vec![string("a")]];
        assert_eq!(by_uuid, expected);
        assert_eq!(by_indexed_uuid, expected);
        assert_eq!(by_reversed_uuid, expected);
        assert_eq!(by_uuid_list, expected);
        assert_eq!(by_bytea, expected);
        assert_eq!(by_timestamp, expected);
    }
}

mod sql_float_integer_casts {
    use cassie::types::Value;

    use super::support_sql_fixture::sql_fixture;

    #[test]
    fn should_round_float_to_nearest_integer_when_casting() {
        // Arrange
        let fixture = sql_fixture(
            "cast_float_rounds",
            &[
                "CREATE TABLE d (id INT, price FLOAT)",
                "INSERT INTO d (id, price) VALUES (1, 19.99)",
                "INSERT INTO d (id, price) VALUES (2, 5.0)",
                "INSERT INTO d (id, price) VALUES (3, 2.5)",
                "INSERT INTO d (id, price) VALUES (4, -3.5)",
            ],
        );

        // Act
        let ints = fixture.rows("SELECT CAST(price AS INT) FROM d ORDER BY id");
        let bigints = fixture.rows("SELECT CAST(price AS BIGINT) FROM d WHERE id = 1");
        let smallints = fixture.rows("SELECT CAST(price AS SMALLINT) FROM d WHERE id = 4");

        // Assert
        assert_eq!(
            ints,
            vec![
                vec![Value::Int64(20)],
                vec![Value::Int64(5)],
                vec![Value::Int64(2)],
                vec![Value::Int64(-4)],
            ]
        );
        assert_eq!(bigints, vec![vec![Value::Int64(20)]]);
        assert_eq!(smallints, vec![vec![Value::Int64(-4)]]);
    }

    #[test]
    fn should_reject_float_cast_outside_integer_range() {
        // Arrange
        let fixture = sql_fixture(
            "cast_float_out_of_range",
            &[
                "CREATE TABLE r (price FLOAT)",
                "INSERT INTO r (price) VALUES (1.0e10)",
            ],
        );

        // Act
        let failed = fixture.execute("SELECT CAST(price AS INT) FROM r").is_err();

        // Assert
        assert!(failed, "a float beyond int4 must not cast to INT");
    }
}

mod typed_value_consistency {
    use super::support_sql_fixture::sql_fixture;
    use cassie::types::Value;

    #[test]
    fn should_emit_canonical_values_from_typed_casts() {
        // Arrange
        let fixture = sql_fixture("canonical_typed_casts", &[
            "CREATE TABLE typed_casts (id INT, u UUID, b BYTEA)",
            "INSERT INTO typed_casts VALUES (1, 'a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11', '\\xdeadbeef')",
        ]);

        // Act
        let rows = fixture.rows(
            "SELECT CAST('A0EEBC999C0B4EF8BB6D6BB9BD380A11' AS UUID), CAST('\\xDEADBEEF' AS BYTEA)",
        );

        // Assert
        assert_eq!(
            rows,
            vec![vec![
                Value::String("a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11".into()),
                Value::String("\\xdeadbeef".into())
            ]]
        );
        for predicate in [
            "u = CAST('A0EEBC999C0B4EF8BB6D6BB9BD380A11' AS UUID)",
            "b = CAST('\\xDEADBEEF' AS BYTEA)",
        ] {
            assert_eq!(
                fixture.rows(&format!("SELECT id FROM typed_casts WHERE {predicate}")),
                vec![vec![Value::Int64(1)]]
            );
        }
    }

    #[test]
    fn should_apply_canonical_typed_literals_to_mutation_predicates() {
        // Arrange
        let fixture = sql_fixture(
            "canonical_typed_dml",
            &["CREATE TABLE typed_dml (id INT, u UUID, b BYTEA)"],
        );
        for predicate in [
            "u = 'A0EEBC999C0B4EF8BB6D6BB9BD380A11'",
            "u IN ('A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11')",
            "u BETWEEN 'A0EEBC999C0B4EF8BB6D6BB9BD380A11' AND 'A0EEBC999C0B4EF8BB6D6BB9BD380A11'",
            "b = '\\xDEADBEEF'",
            "b IN ('\\xDEADBEEF')",
            "b BETWEEN '\\xDEADBEEF' AND '\\xDEADBEEF'",
        ] {
            fixture.execute("INSERT INTO typed_dml VALUES (1, 'a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11', '\\xdeadbeef')").expect("seed");
            assert_eq!(
                fixture.rows(&format!("SELECT id FROM typed_dml WHERE {predicate}")),
                vec![vec![Value::Int64(1)]]
            );

            // Act
            fixture
                .execute(&format!("UPDATE typed_dml SET id = 2 WHERE {predicate}"))
                .expect("update");

            // Assert
            assert_eq!(
                fixture.rows("SELECT id FROM typed_dml"),
                vec![vec![Value::Int64(2)]]
            );
            fixture
                .execute(&format!("DELETE FROM typed_dml WHERE {predicate}"))
                .expect("delete");
            assert_eq!(
                fixture.rows("SELECT id FROM typed_dml"),
                [] as [Vec<Value>; 0]
            );
        }
    }
    #[test]
    fn should_reject_incompatible_coalesce_types_before_execution() {
        // Arrange
        let fixture = sql_fixture(
            "coalesce_result_types",
            &[
                "CREATE TABLE coalesce_types (id INT, flag BOOLEAN, name TEXT, score INT)",
                "CREATE TABLE coalesce_other (id INT)",
                "INSERT INTO coalesce_other VALUES (2)",
                "INSERT INTO coalesce_types VALUES (1, true, 'x', 5), (2, NULL, 'y', 0)",
                "CREATE FUNCTION bool_identity(x BOOLEAN) RETURNS BOOLEAN AS \"x\"",
            ],
        );

        // Act
        for expression in [
            "COALESCE(flag, name)",
            "COALESCE(coalesce_types.flag, coalesce_types.name)",
            "COALESCE(flag, score)",
            "COALESCE(flag, ABS(score))",
            "COALESCE(bool_identity(flag), name)",
            "CASE WHEN id = 1 THEN true ELSE COALESCE(flag, name) END",
            "COALESCE(CASE WHEN id = 1 THEN flag ELSE NULL END, name)",
        ] {
            let result = fixture.execute(&format!(
                "SELECT {expression} FROM coalesce_types ORDER BY id"
            ));

            // Assert
            assert!(
                result.is_err(),
                "incompatible result accepted: {expression}"
            );
        }
        assert!(fixture.execute("SELECT COALESCE(coalesce_types.flag, coalesce_types.name) FROM coalesce_types JOIN coalesce_other ON coalesce_types.id = coalesce_other.id").is_err(), "qualified join COALESCE must reject incompatible types");
        for sql in [
            "UPDATE coalesce_types SET score = 99 RETURNING COALESCE(flag, name)",
            "DELETE FROM coalesce_types RETURNING COALESCE(flag, score)",
            "INSERT INTO coalesce_types VALUES (3, true, 'z', 7) RETURNING COALESCE(flag, name)",
        ] {
            assert!(
                fixture.execute(sql).is_err(),
                "incompatible RETURNING accepted: {sql}"
            );
        }
        assert_eq!(
            fixture.rows("SELECT COALESCE(flag, false) FROM coalesce_types ORDER BY id"),
            vec![vec![Value::Bool(true)], vec![Value::Bool(false)]]
        );
        assert_eq!(
            fixture.rows("SELECT COALESCE(NULL, score, 1.5) FROM coalesce_types ORDER BY id"),
            vec![vec![Value::Float64(5.0)], vec![Value::Float64(0.0)]]
        );
    }
    #[test]
    fn should_validate_bound_parameter_coalesce_result_types() {
        // Arrange
        let fixture = sql_fixture(
            "coalesce_parameter_types",
            &[
                "CREATE TABLE parameter_coalesce (flag BOOLEAN)",
                "CREATE TABLE uuid_parameter_coalesce (value UUID)",
                "CREATE TABLE array_parameter_coalesce (value INT[])",
                "INSERT INTO array_parameter_coalesce VALUES (NULL)",
                "INSERT INTO uuid_parameter_coalesce VALUES (NULL)",
                "INSERT INTO parameter_coalesce VALUES (NULL)",
            ],
        );

        // Act
        for query in [
            "SELECT COALESCE(flag, $1) FROM parameter_coalesce",
            "SELECT value FROM (SELECT COALESCE(flag, $1) AS value FROM parameter_coalesce) AS nested",
            "WITH values_cte AS (SELECT COALESCE(flag, $1) AS value FROM parameter_coalesce) SELECT value FROM values_cte",
            "WITH seed AS (SELECT flag FROM parameter_coalesce), values_cte AS (SELECT COALESCE(flag, $1) AS value FROM seed) SELECT value FROM values_cte",
            "SELECT flag FROM parameter_coalesce UNION ALL SELECT COALESCE(flag, $1) FROM parameter_coalesce",
            "UPDATE parameter_coalesce SET flag = true RETURNING COALESCE(flag, $1)",
        ] {
            let incompatible = fixture.cassie.execute_sql(&fixture.session, query, vec![Value::String("text".into())]);

            // Assert
            assert!(incompatible.is_err(), "boolean/text parameters accepted: {query}");
        }
        let compatible = fixture
            .cassie
            .execute_sql(
                &fixture.session,
                "SELECT COALESCE(flag, $1) FROM parameter_coalesce",
                vec![Value::Bool(false)],
            )
            .expect("compatible parameter");
        assert_eq!(compatible.rows, vec![vec![Value::Bool(false)]]);
        let chained = fixture.cassie.execute_sql(&fixture.session,
            "WITH seed AS (SELECT flag FROM parameter_coalesce), values_cte AS (SELECT COALESCE(flag, $1) AS value FROM seed) SELECT value FROM values_cte",
            vec![Value::Bool(false)]).expect("compatible chained CTE");
        assert_eq!(chained.rows, vec![vec![Value::Bool(false)]]);
        let canonical = "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11";
        let uuid_result = fixture
            .cassie
            .execute_sql(
                &fixture.session,
                "SELECT COALESCE(value, $1) FROM uuid_parameter_coalesce",
                vec![Value::String(canonical.into())],
            )
            .expect("UUID string parameter");
        assert_eq!(
            uuid_result.rows,
            vec![vec![Value::String(canonical.into())]]
        );
        let array = serde_json::json!([1, 2]);
        let array_result = fixture
            .cassie
            .execute_sql(
                &fixture.session,
                "SELECT COALESCE(value, $1) FROM array_parameter_coalesce",
                vec![Value::Json(array.clone())],
            )
            .expect("array parameter");
        assert_eq!(array_result.rows, vec![vec![Value::Json(array)]]);
    }
}

mod sql_separator_consistency {
    use cassie::sql::ast::QueryStatement;
    use cassie::sql::parse_statement;

    const QUERY_SEPARATOR_PAIRS: &[(&str, &str)] = &[
        ("SELECT * FROM (SELECT a FROM t) AS x", "SELECT * FROM (SELECT a FROM t)/**/AS/**/x"),
        ("SELECT l.a,x.a FROM l JOIN LATERAL (SELECT a FROM t) AS x ON true", "SELECT l.a,x.a FROM l JOIN LATERAL\n(SELECT a FROM t) AS\nx ON true"),
        ("SELECT * FROM (SELECT a FROM t) AS x", "SELECT * FROM (SELECT a /* ) ' */ FROM t) AS x"),

            ("SELECT CASE WHEN a=1 THEN 2 ELSE 3 END FROM t", "SELECT CASE/*END*/WHEN a=1 THEN/*WHEN*/2 ELSE 3 END FROM t"),
            ("SELECT CAST(a AS INT) FROM t", "SELECT CAST/*separator*/(a AS\nINT) FROM t"),

            ("SELECT DISTINCT ON(a) a FROM t ORDER BY a DESC NULLS FIRST", "SELECT DISTINCT\nON(a) a FROM t ORDER\nBY a\tDESC NULLS\nFIRST"),
            ("SELECT first_value(a) OVER (PARTITION BY b ORDER BY a ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) FROM t", "SELECT first_value(a) OVER (PARTITION\nBY b ORDER\tBY a ROWS BETWEEN\nUNBOUNDED\tPRECEDING AND CURRENT\nROW) FROM t"),

            (
                "INSERT INTO t(a) VALUES(1) ON CONFLICT(a) DO UPDATE SET a=excluded.a",
                "INSERT\nINTO t(a) VALUES(1) ON\nCONFLICT(a) DO UPDATE\nSET a=excluded.a",
            ),
            ("DELETE FROM t WHERE a=1", "DELETE\nFROM t WHERE a=1"),
            (
                "SELECT '\\' AS a FROM t ORDER BY a",
                "SELECT '\\' AS a FROM t ORDER\nBY a",
            ),
            (
                "SELECT a FROM t",
                "SELECT a FROM t; -- trailing statement comment",
            ),
            (
                "SELECT a FROM t",
                "/*leading SELECT*/ SELECT a FROM/*source*/t -- trailing ORDER BY",
            ),
            (
                "SELECT a,b FROM t",
                "SELECT /*comma ,*/a,/*ignored ,*/b FROM t",
            ),
            (
                "SELECT a FROM t WHERE a BETWEEN 1 AND 2 AND b BETWEEN 3 AND 4",
                "SELECT a FROM t WHERE a BETWEEN 1\nAND 2\tAND b BETWEEN 3/*gap*/AND 4",
            ),
            ("SELECT a FROM t ORDER BY a", "SELECT\na FROM t ORDER\nBY a"),
            (
                "SELECT a FROM t GROUP BY a",
                "SELECT a FROM t GROUP\t  BY a",
            ),
            (
                "SELECT a FROM t WHERE a = 1 AND b = 2",
                "SELECT a FROM t WHERE a = 1\nAND\tb = 2",
            ),
            (
                "SELECT a FROM t WHERE a BETWEEN 1 AND 2",
                "SELECT a FROM t WHERE a BETWEEN 1\nAND 2",
            ),
            (
                "SELECT a FROM t WHERE a IS NOT NULL",
                "SELECT a FROM t WHERE a IS\tNOT\nNULL",
            ),
            (
                "SELECT a FROM t WHERE a NOT IN (1,2)",
                "SELECT a FROM t WHERE a NOT\n IN (1,2)",
            ),
            (
                "SELECT a FROM t ORDER BY a",
                "SELECT/*SELECT WHERE*/a FROM t ORDER/*nested /*inner*/ comment*/BY a",
            ),
            (
                "SELECT l.a FROM l LEFT JOIN r ON l.a = r.a",
                "SELECT l.a FROM l LEFT\nJOIN r\nON\tl.a = r.a",
            ),
            (
                "SELECT a FROM t UNION ALL SELECT a FROM t",
                "SELECT a FROM t UNION/*separator*/ALL SELECT a FROM t",
            ),
            (
                "SELECT a FROM t WHERE a = 1 OR b = 2",
                "SELECT a FROM t WHERE a = 1-- OR false\nOR b = 2",
            ),
        ];

    #[test]
    fn should_preserve_query_semantics_across_sql_separators() {
        // Arrange
        for &(canonical, formatted) in QUERY_SEPARATOR_PAIRS {
            // Act
            let expected = parse_statement(canonical).expect("canonical query");
            let actual = parse_statement(formatted).expect(formatted);
            // Assert
            assert_eq!(
                format!("{:?}", actual.statement),
                format!("{:?}", expected.statement),
                "{formatted}"
            );
            assert_eq!(actual.raw_sql, formatted);
        }
    }

    #[test]
    fn should_parse_explicit_join_kind_spellings() {
        // Arrange
        for (explicit, canonical) in [
            ("INNER JOIN", "JOIN"),
            ("LEFT OUTER JOIN", "LEFT JOIN"),
            ("RIGHT OUTER JOIN", "RIGHT JOIN"),
            ("FULL OUTER JOIN", "FULL JOIN"),
        ] {
            // Act
            let expected =
                parse_statement(&format!("SELECT l.a FROM l {canonical} r ON l.a = r.a")).unwrap();
            let actual = parse_statement(&format!("SELECT l.a FROM l {explicit} r ON l.a = r.a"))
                .expect(explicit);
            // Assert
            assert_eq!(
                format!("{:?}", actual.statement),
                format!("{:?}", expected.statement)
            );
        }
    }

    #[test]
    fn should_preserve_quoted_sql_bytes_while_matching_separators() {
        // Arrange
        let sql = "SELECT 'café ORDER\nBY -- /* */' AS \"GROUP BY\" FROM t ORDER\nBY \"GROUP BY\"";
        let routine = "CREATE FUNCTION preserved_body() RETURNS TEXT AS \"'ORDER\nBY -- /* */'\"";
        let view = "CREATE VIEW v AS SELECT 'ORDER\nBY' AS a FROM t ORDER\nBY a";
        // Act
        let parsed = parse_statement(sql).expect("quoted query");
        let routine = parse_statement(routine).expect("quoted routine body");
        let stored = parse_statement(view).expect("stored view");
        // Assert
        assert_eq!(parsed.raw_sql, sql);
        assert!(routine.raw_sql.contains("ORDER\nBY -- /* */"));
        let QueryStatement::CreateView(statement) = stored.statement else {
            panic!("view expected");
        };
        assert_eq!(
            statement.query,
            "SELECT 'ORDER\nBY' AS a FROM t ORDER\nBY a"
        );
    }
    #[test]
    fn should_preserve_nested_query_sql_text() {
        // Arrange
        let nested = "SELECT a /*preserved query comment*/ FROM t";
        // Act
        let parsed =
            parse_statement(&format!("SELECT 1 WHERE EXISTS({nested})")).expect("nested query");
        // Assert
        let QueryStatement::Select(select) = parsed.statement else {
            panic!("SELECT expected");
        };
        let Some(cassie::sql::ast::Expr::Exists(query)) = select.filter else {
            panic!("EXISTS expected");
        };
        assert_eq!(query.raw_sql, nested);
    }
    #[test]
    fn should_parse_supported_query_tokens_with_each_separator_form() {
        // Arrange
        let queries=[
            "SELECT a FROM t WHERE a BETWEEN 1 AND 2 AND a IS NOT NULL ORDER BY a DESC NULLS LAST LIMIT 1 OFFSET 0",
            "SELECT l.a FROM l LEFT OUTER JOIN r ON l.a = r.a",
            "SELECT l.a,x.a FROM l JOIN LATERAL (SELECT a FROM t) AS x ON true",
            "SELECT CASE WHEN a = 1 THEN 2 ELSE 3 END FROM t",
            "SELECT CAST(a AS INT) FROM t",
            "SELECT first_value(a) OVER (PARTITION BY b ORDER BY a ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) FROM t",
            "INSERT INTO t(a) VALUES(1) ON CONFLICT(a) DO UPDATE SET a=excluded.a",
            "UPDATE t SET a=1 WHERE a=2",
            "DELETE FROM t WHERE a=1",
        ];
        for canonical in queries {
            let expected = parse_statement(canonical).expect("canonical query");
            for separator in [
                "  ",
                "\n",
                "\t",
                "/**/",
                "/*( SELECT JOIN AND )*/",
                "/* ) ' , = AND */",
                "\u{2003}\t/* JOIN */\n",
            ] {
                let formatted = canonical.replace(' ', separator);
                // Act
                let actual = parse_statement(&formatted).expect(&formatted);
                // Assert
                assert_eq!(
                    format!("{:?}", actual.statement),
                    format!("{:?}", expected.statement),
                    "{formatted}"
                );
                assert_eq!(actual.raw_sql, formatted);
            }
        }
    }
    #[test]
    fn should_parse_cte_keyword_separators_without_losing_recursive_scope() {
        // Arrange
        let sql="WITH/* ) ' */RECURSIVE/*gap*/seq/*header*/(n/*alias*/)/*head*/AS/*body*/(SELECT 1/* ) ' */ UNION/*gap*/ALL SELECT n+1 FROM seq WHERE n<2) SELECT\nn FROM seq";
        // Act
        let parsed = parse_statement(sql).expect("recursive CTE separators");
        // Assert
        assert_eq!(parsed.raw_sql, sql);
        let QueryStatement::Select(select) = parsed.statement else {
            panic!("SELECT expected");
        };
        assert!(select.recursive);
        assert_eq!(select.ctes[0].name, "seq");
        assert_eq!(select.ctes[0].aliases, ["n"]);
    }
}
