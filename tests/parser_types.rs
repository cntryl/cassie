// Consolidated integration suite: parser_types.
// Shared fixtures live in tests/support; former test targets remain named modules.

#[path = "support/pgwire.rs"]
mod support_pgwire;

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
            QuerySource::Collection("docs".to_string())
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
            QuerySource::Collection("docs_cte".to_string())
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
            QuerySource::Collection("second".to_string())
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
            QuerySource::Collection("docs_cte".to_string())
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
        assert!(statement.fields.is_empty());
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
        assert!(matches!(*right, QuerySource::Collection(ref name) if name == "regions"));
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
        assert_eq!(selected.rows[0][4], Value::String("2026-06-18T12:34:56Z".to_string()));
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
