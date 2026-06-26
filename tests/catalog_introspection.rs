use cassie::app::Cassie;
use cassie::config::CassieRuntimeConfig;
use cassie::types::{DataType, Value};
use std::path::PathBuf;
use uuid::Uuid;

fn with_fallback() {
    if std::env::var("CASSIE_EMBEDDINGS_PROVIDER").is_err() {
        std::env::set_var("CASSIE_EMBEDDINGS_PROVIDER", "fallback");
    }
}

fn data_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("cassie-catalog-{name}-{}", Uuid::new_v4()))
}

#[test]
fn should_list_user_tables_through_information_schema() {
    // Arrange
    with_fallback();
    let path = data_dir("tables");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(
            &path,
            cassie::config::CassieRuntimeConfig {
                user: "postgres".to_string(),
                ..cassie::config::CassieRuntimeConfig::default()
            },
        )
        .unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE catalog_tables_docs (title TEXT)",
                vec![],
            )

.unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT table_name FROM information_schema.tables WHERE table_name = 'catalog_tables_docs'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("catalog_tables_docs".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_list_columns_through_information_schema_after_restart() {
    // Arrange
    with_fallback();
    let path = data_dir("columns_restart");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(
            &path,
            cassie::config::CassieRuntimeConfig {
                user: "postgres".to_string(),
                ..cassie::config::CassieRuntimeConfig::default()
            },
        )
        .unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE catalog_columns_docs (title TEXT, score INT)",
                vec![],
            )

.unwrap();
        drop(cassie);

        let restarted = Cassie::new_with_data_dir(&path).unwrap();
        restarted.startup().unwrap();
        let session = restarted.create_session("tester", None);

        // Act
        let selected = restarted
            .execute_sql(
                &session,
                "SELECT column_name, data_type FROM information_schema.columns WHERE table_name = 'catalog_columns_docs' ORDER BY ordinal_position",
                vec![],
            )

.unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![
                    Value::String("title".to_string()),
                    Value::String("text".to_string())
                ],
                vec![
                    Value::String("score".to_string()),
                    Value::String("int".to_string())
                ]
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_list_indexes_through_pg_catalog() {
    // Arrange
    with_fallback();
    let path = data_dir("indexes");
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
                "CREATE TABLE catalog_index_docs (email TEXT)",
                vec![],
            )

.unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE UNIQUE INDEX catalog_email_idx ON catalog_index_docs USING btree (email)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT indexname FROM pg_catalog.pg_indexes WHERE tablename = 'catalog_index_docs'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("catalog_email_idx".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_list_primary_key_index_through_pg_catalog() {
    // Arrange
    with_fallback();
    let path = data_dir("primary_key_index");
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
                "CREATE TABLE catalog_primary_key_docs (id INT PRIMARY KEY, title TEXT)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT indexname, indexdef FROM pg_catalog.pg_indexes WHERE tablename = 'catalog_primary_key_docs'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(selected.rows.len(), 1);
        assert_eq!(
            selected.rows[0][0],
            Value::String("catalog_primary_key_docs_pkey".to_string())
        );
        assert_eq!(
            selected.rows[0][1],
            Value::String(
                "CREATE UNIQUE INDEX catalog_primary_key_docs_pkey ON catalog_primary_key_docs (id)"
                    .to_string()
            )
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_list_composite_indexes_through_pg_catalog() {
    // Arrange
    with_fallback();
    let path = data_dir("composite_indexes");
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
                "CREATE TABLE catalog_composite_index_docs (title TEXT, score INT)",
                vec![],
            )

.unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE INDEX catalog_title_score_idx ON catalog_composite_index_docs USING btree (title, score)",
                vec![],
            )
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT indexname, indexdef FROM pg_catalog.pg_indexes WHERE tablename = 'catalog_composite_index_docs'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![
                Value::String("catalog_title_score_idx".to_string()),
                Value::String(
                    "CREATE INDEX catalog_title_score_idx ON catalog_composite_index_docs (title, score)"
                        .to_string()
                ),
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_report_column_store_storage_metadata_after_restart() {
    // Arrange
    with_fallback();
    let path = data_dir("column_store_storage_restart");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let mut config = CassieRuntimeConfig {
            user: "postgres".to_string(),
            ..CassieRuntimeConfig::default()
        };
        config.limits.experimental_column_store_enabled = true;

        let cassie = Cassie::new_with_data_dir_and_config(&path, config.clone()).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE catalog_column_store_docs (doc_id TEXT, title TEXT, score INT) WITH (storage = column_store)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO catalog_column_store_docs (doc_id, title, score) VALUES ('d1', 'alpha', 7)",
                vec![],
            )
            .unwrap();
        drop(cassie);

        // Act
        let restarted = Cassie::new_with_data_dir_and_config(&path, config).unwrap();
        restarted.startup().unwrap();
        let session = restarted.create_session("tester", None);
        let storage = restarted
            .execute_sql(
                &session,
                "SELECT tablename, storage_mode, storage_version FROM pg_catalog.pg_table_storage WHERE tablename = 'catalog_column_store_docs'",
                vec![],
            )
            .unwrap();
        let selected = restarted
            .execute_sql(
                &session,
                "SELECT doc_id, title, score FROM catalog_column_store_docs",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            storage.rows,
            vec![vec![
                Value::String("catalog_column_store_docs".to_string()),
                Value::String("column-store".to_string()),
                Value::Int64(1),
            ]]
        );
        assert_eq!(
            selected.rows,
            vec![vec![
                Value::String("d1".to_string()),
                Value::String("alpha".to_string()),
                Value::Int64(7),
            ]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_list_namespaces_through_pg_catalog() {
    // Arrange
    with_fallback();
    let path = data_dir("namespaces");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(&session, "CREATE SCHEMA analytics", vec![])
            .unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT nspname FROM pg_catalog.pg_namespace ORDER BY nspname",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![Value::String("analytics".to_string())],
                vec![Value::String("information_schema".to_string())],
                vec![Value::String("pg_catalog".to_string())],
                vec![Value::String("public".to_string())],
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_list_constraints_through_information_schema() {
    // Arrange
    with_fallback();
    let path = data_dir("constraints");
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
                "CREATE TABLE catalog_constraint_docs (email TEXT UNIQUE, score INT CHECK (score >= 0))",
                vec![],
            )

.unwrap();

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT constraint_type FROM information_schema.table_constraints WHERE table_name = 'catalog_constraint_docs' ORDER BY constraint_type",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![Value::String("CHECK".to_string())],
                vec![Value::String("UNIQUE".to_string())]
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_list_named_foreign_key_metadata_through_catalog_views() {
    // Arrange
    with_fallback();
    let path = data_dir("named_fk_metadata");
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
                r#"CREATE TABLE "catalog_named_fk_parents" (
                    "id" INT,
                    CONSTRAINT "catalog_named_fk_parents_pkey" PRIMARY KEY ("id")
                )"#,
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                r#"CREATE TABLE "catalog_named_fk_children" (
                    "id" INT,
                    "parent_id" INT,
                    CONSTRAINT "catalog_named_fk_children_pkey" PRIMARY KEY ("id")
                )"#,
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                r#"ALTER TABLE "catalog_named_fk_children"
                    ADD CONSTRAINT "catalog_named_fk_children_parent_fkey"
                    FOREIGN KEY ("parent_id") REFERENCES "catalog_named_fk_parents"("id")
                    ON DELETE CASCADE ON UPDATE CASCADE"#,
                vec![],
            )
            .unwrap();

        // Act
        let table_constraints = cassie
            .execute_sql(
                &session,
                "SELECT constraint_name, constraint_type FROM information_schema.table_constraints WHERE table_name = 'catalog_named_fk_children' ORDER BY constraint_name",
                vec![],
            )
            .unwrap();
        let key_usage = cassie
            .execute_sql(
                &session,
                "SELECT constraint_name, column_name, ordinal_position FROM information_schema.key_column_usage WHERE table_name = 'catalog_named_fk_children' ORDER BY constraint_name",
                vec![],
            )
            .unwrap();
        let referential = cassie
            .execute_sql(
                &session,
                "SELECT constraint_name, unique_constraint_name, update_rule, delete_rule FROM information_schema.referential_constraints WHERE constraint_name = 'catalog_named_fk_children_parent_fkey'",
                vec![],
            )
            .unwrap();
        let pg_constraint = cassie
            .execute_sql(
                &session,
                "SELECT conname, contype FROM pg_catalog.pg_constraint WHERE conrelid = 'catalog_named_fk_children' ORDER BY conname",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            table_constraints.rows,
            vec![
                vec![
                    Value::String("catalog_named_fk_children_parent_fkey".to_string()),
                    Value::String("FOREIGN KEY".to_string())
                ],
                vec![
                    Value::String("catalog_named_fk_children_pkey".to_string()),
                    Value::String("PRIMARY KEY".to_string())
                ],
            ]
        );
        assert_eq!(
            key_usage.rows,
            vec![
                vec![
                    Value::String("catalog_named_fk_children_parent_fkey".to_string()),
                    Value::String("parent_id".to_string()),
                    Value::Int64(1)
                ],
                vec![
                    Value::String("catalog_named_fk_children_pkey".to_string()),
                    Value::String("id".to_string()),
                    Value::Int64(1)
                ],
            ]
        );
        assert_eq!(
            referential.rows,
            vec![vec![
                Value::String("catalog_named_fk_children_parent_fkey".to_string()),
                Value::String("catalog_named_fk_parents_pkey".to_string()),
                Value::String("CASCADE".to_string()),
                Value::String("CASCADE".to_string())
            ]]
        );
        assert_eq!(
            pg_constraint.rows,
            vec![
                vec![
                    Value::String("catalog_named_fk_children_id_n".to_string()),
                    Value::String("n".to_string())
                ],
                vec![
                    Value::String("catalog_named_fk_children_parent_fkey".to_string()),
                    Value::String("f".to_string())
                ],
                vec![
                    Value::String("catalog_named_fk_children_pkey".to_string()),
                    Value::String("p".to_string())
                ],
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_return_admin_role_for_pg_roles_catalog_view() {
    // Arrange
    with_fallback();
    let path = data_dir("empty_pg_roles");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let selected = cassie
            .execute_sql(&session, "SELECT rolname FROM pg_catalog.pg_roles", vec![])
            .unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![vec![Value::String("postgres".to_string())]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_list_supported_types_through_pg_catalog_type_view() {
    // Arrange
    with_fallback();
    let path = data_dir("pg_type");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);

        // Act
        let selected = cassie
            .execute_sql(
                &session,
                "SELECT typname, oid, typelem, typnamespace FROM pg_catalog.pg_type WHERE typname IN ('smallint', 'bigint', 'bytea', 'char(1)', 'varchar(8)', 'int', 'int[]', 'vector(2)', 'text', 'bytea[]') ORDER BY typname",
                vec![],
            )

.unwrap();

        // Assert
        assert_eq!(
            selected.rows,
            vec![
                vec![
                    Value::String("bigint".to_string()),
                    Value::Int64(DataType::BigInt.type_oid()),
                    Value::Int64(0),
                    Value::String("pg_catalog".to_string())
                ],
                vec![
                    Value::String("bytea".to_string()),
                    Value::Int64(DataType::Bytea.type_oid()),
                    Value::Int64(0),
                    Value::String("pg_catalog".to_string())
                ],
                vec![
                    Value::String("bytea[]".to_string()),
                    Value::Int64(DataType::Array(Box::new(DataType::Bytea)).type_oid()),
                    Value::Int64(DataType::Bytea.type_oid()),
                    Value::String("pg_catalog".to_string())
                ],
                vec![
                    Value::String("char(1)".to_string()),
                    Value::Int64(DataType::Char { length: Some(1) }.type_oid()),
                    Value::Int64(0),
                    Value::String("pg_catalog".to_string())
                ],
                vec![
                    Value::String("int".to_string()),
                    Value::Int64(DataType::Int.type_oid()),
                    Value::Int64(0),
                    Value::String("pg_catalog".to_string())
                ],
                vec![
                    Value::String("int[]".to_string()),
                    Value::Int64(DataType::Array(Box::new(DataType::Int)).type_oid()),
                    Value::Int64(DataType::Int.type_oid()),
                    Value::String("pg_catalog".to_string())
                ],
                vec![
                    Value::String("smallint".to_string()),
                    Value::Int64(DataType::SmallInt.type_oid()),
                    Value::Int64(0),
                    Value::String("pg_catalog".to_string())
                ],
                vec![
                    Value::String("text".to_string()),
                    Value::Int64(DataType::Text.type_oid()),
                    Value::Int64(0),
                    Value::String("pg_catalog".to_string())
                ],
                vec![
                    Value::String("varchar(8)".to_string()),
                    Value::Int64(DataType::Varchar { length: Some(8) }.type_oid()),
                    Value::Int64(0),
                    Value::String("pg_catalog".to_string())
                ],
                vec![
                    Value::String("vector(2)".to_string()),
                    Value::Int64(DataType::Vector(2).type_oid()),
                    Value::Int64(0),
                    Value::String("pg_catalog".to_string())
                ],
            ]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_list_user_defined_views_through_catalog_views() {
    // Arrange
    with_fallback();
    let path = data_dir("views");
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
                "CREATE TABLE catalog_views_docs (title TEXT, score INT)",
                vec![],
            )

.unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE VIEW catalog_views_ready AS SELECT title, score FROM catalog_views_docs",
                vec![],
            )
            .unwrap();

        // Act
        let tables = cassie
            .execute_sql(
                &session,
                "SELECT table_type FROM information_schema.tables WHERE table_name = 'catalog_views_ready'",
                vec![],
            )
            .unwrap();
        let views = cassie
            .execute_sql(
                &session,
                "SELECT table_name FROM information_schema.views WHERE table_name = 'catalog_views_ready'",
                vec![],
            )
            .unwrap();
        let classes = cassie
            .execute_sql(
                &session,
                "SELECT relkind FROM pg_catalog.pg_class WHERE relname = 'catalog_views_ready'",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(tables.rows, vec![vec![Value::String("VIEW".to_string())]]);
        assert_eq!(views.rows, vec![vec![Value::String("catalog_views_ready".to_string())]]);
        assert_eq!(classes.rows, vec![vec![Value::String("v".to_string())]]);

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_support_pgadmin_browser_workflow_catalog_queries() {
    // Arrange
    with_fallback();
    let path = data_dir("pgadmin_browser");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", Some("catalogdb".to_string()));
        cassie
            .execute_sql(
                &session,
                r#"CREATE TABLE "catalog_pgadmin_docs" (
                    "id" INT DEFAULT 1,
                    "title" VARCHAR(32) NOT NULL,
                    "score" INT,
                    CONSTRAINT "catalog_pgadmin_docs_pkey" PRIMARY KEY ("id"),
                    CONSTRAINT "catalog_pgadmin_docs_score_check" CHECK (score >= 0)
                )"#,
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                r#"CREATE INDEX "catalog_pgadmin_docs_title_idx"
                    ON "catalog_pgadmin_docs" ("title")"#,
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE VIEW catalog_pgadmin_ready AS SELECT title, score FROM catalog_pgadmin_docs",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO catalog_pgadmin_docs (id, title, score) VALUES (1, 'alpha', 9)",
                vec![],
            )
            .unwrap();

        // Act
        let namespace = cassie
            .execute_sql(
                &session,
                "SELECT oid, nspname, pg_catalog.pg_get_userbyid(nspowner), pg_catalog.has_schema_privilege(nspname, 'USAGE') FROM pg_catalog.pg_namespace WHERE nspname = 'public'",
                vec![],
            )
            .unwrap();
        let classes = cassie
            .execute_sql(
                &session,
                "SELECT relname, relkind, relnamespace_oid, relhasindex, relpersistence, pg_catalog.quote_ident(relname), pg_catalog.has_table_privilege(relname, 'SELECT'), pg_catalog.pg_table_is_visible(oid) FROM pg_catalog.pg_class WHERE relname IN ('catalog_pgadmin_docs', 'catalog_pgadmin_docs_title_idx', 'catalog_pgadmin_ready') ORDER BY relname",
                vec![],
            )
            .unwrap();
        let columns = cassie
            .execute_sql(
                &session,
                "SELECT attname, attnum, pg_catalog.format_type(atttypid, atttypmod), attnotnull, atthasdef, attrelid_oid, attisdropped FROM pg_catalog.pg_attribute WHERE attrelid = 'catalog_pgadmin_docs' ORDER BY attnum",
                vec![],
            )
            .unwrap();
        let defaults = cassie
            .execute_sql(
                &session,
                "SELECT pg_catalog.pg_get_expr(adsrc, adrelid) FROM pg_catalog.pg_attrdef WHERE adrelid = 'catalog_pgadmin_docs' ORDER BY adnum",
                vec![],
            )
            .unwrap();
        let indexes = cassie
            .execute_sql(
                &session,
                "SELECT indexrelid, indrelid, indexrelid_oid, indrelid_oid, indisunique, indisprimary, indisvalid FROM pg_catalog.pg_index WHERE indrelid = 'catalog_pgadmin_docs' ORDER BY indexrelid",
                vec![],
            )
            .unwrap();
        let constraints = cassie
            .execute_sql(
                &session,
                "SELECT conname, conrelid, conrelid_oid, contype, conkey, convalidated FROM pg_catalog.pg_constraint WHERE conrelid = 'catalog_pgadmin_docs' ORDER BY conname",
                vec![],
            )
            .unwrap();
        let view = cassie
            .execute_sql(
                &session,
                "SELECT table_name, view_definition FROM information_schema.views WHERE table_name = 'catalog_pgadmin_ready'",
                vec![],
            )
            .unwrap();
        let table_data = cassie
            .execute_sql(
                &session,
                "SELECT title, score FROM catalog_pgadmin_docs ORDER BY title",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(namespace.rows.len(), 1);
        assert_eq!(namespace.rows[0][1], Value::String("public".to_string()));
        assert_eq!(namespace.rows[0][2], Value::String("postgres".to_string()));
        assert_eq!(namespace.rows[0][3], Value::Bool(true));
        let Value::Int64(public_oid) = namespace.rows[0][0] else {
            panic!("public namespace oid should be numeric");
        };

        assert_eq!(
            classes.rows,
            vec![
                vec![
                    Value::String("catalog_pgadmin_docs".to_string()),
                    Value::String("r".to_string()),
                    Value::Int64(public_oid),
                    Value::Bool(true),
                    Value::String("p".to_string()),
                    Value::String("catalog_pgadmin_docs".to_string()),
                    Value::Bool(true),
                    Value::Bool(true),
                ],
                vec![
                    Value::String("catalog_pgadmin_docs_title_idx".to_string()),
                    Value::String("i".to_string()),
                    Value::Int64(public_oid),
                    Value::Bool(false),
                    Value::String("p".to_string()),
                    Value::String("catalog_pgadmin_docs_title_idx".to_string()),
                    Value::Bool(true),
                    Value::Bool(true),
                ],
                vec![
                    Value::String("catalog_pgadmin_ready".to_string()),
                    Value::String("v".to_string()),
                    Value::Int64(public_oid),
                    Value::Bool(false),
                    Value::String("p".to_string()),
                    Value::String("catalog_pgadmin_ready".to_string()),
                    Value::Bool(true),
                    Value::Bool(true),
                ],
            ]
        );
        let Value::Int64(table_oid) = columns.rows[0][5] else {
            panic!("table oid should be numeric");
        };
        assert!(table_oid > 0);
        assert_eq!(
            columns.rows,
            vec![
                vec![
                    Value::String("id".to_string()),
                    Value::Int64(1),
                    Value::String("integer".to_string()),
                    Value::Bool(true),
                    Value::Bool(true),
                    Value::Int64(table_oid),
                    Value::Bool(false),
                ],
                vec![
                    Value::String("title".to_string()),
                    Value::Int64(2),
                    Value::String("character varying(32)".to_string()),
                    Value::Bool(true),
                    Value::Bool(false),
                    Value::Int64(table_oid),
                    Value::Bool(false),
                ],
                vec![
                    Value::String("score".to_string()),
                    Value::Int64(3),
                    Value::String("integer".to_string()),
                    Value::Bool(false),
                    Value::Bool(false),
                    Value::Int64(table_oid),
                    Value::Bool(false),
                ],
            ]
        );
        assert_eq!(defaults.rows, vec![vec![Value::String("1".to_string())]]);
        assert_eq!(indexes.rows.len(), 2);
        assert_eq!(
            indexes.rows.iter().map(|row| row[0].clone()).collect::<Vec<_>>(),
            vec![
                Value::String("catalog_pgadmin_docs_pkey".to_string()),
                Value::String("catalog_pgadmin_docs_title_idx".to_string()),
            ]
        );
        assert!(indexes.rows.iter().all(|row| row[2].as_i64().is_some()));
        assert!(indexes.rows.iter().all(|row| row[3] == Value::Int64(table_oid)));
        assert!(indexes.rows.iter().all(|row| row[6] == Value::Bool(true)));
        assert_eq!(
            constraints.rows,
            vec![
                vec![
                    Value::String("catalog_pgadmin_docs_id_n".to_string()),
                    Value::String("catalog_pgadmin_docs".to_string()),
                    Value::Int64(table_oid),
                    Value::String("n".to_string()),
                    Value::String("1".to_string()),
                    Value::Bool(true),
                ],
                vec![
                    Value::String("catalog_pgadmin_docs_pkey".to_string()),
                    Value::String("catalog_pgadmin_docs".to_string()),
                    Value::Int64(table_oid),
                    Value::String("p".to_string()),
                    Value::String("1".to_string()),
                    Value::Bool(true),
                ],
                vec![
                    Value::String("catalog_pgadmin_docs_score_check".to_string()),
                    Value::String("catalog_pgadmin_docs".to_string()),
                    Value::Int64(table_oid),
                    Value::String("c".to_string()),
                    Value::String("3".to_string()),
                    Value::Bool(true),
                ],
                vec![
                    Value::String("catalog_pgadmin_docs_title_n".to_string()),
                    Value::String("catalog_pgadmin_docs".to_string()),
                    Value::Int64(table_oid),
                    Value::String("n".to_string()),
                    Value::String("2".to_string()),
                    Value::Bool(true),
                ],
            ]
        );
        assert_eq!(
            view.rows,
            vec![vec![
                Value::String("catalog_pgadmin_ready".to_string()),
                Value::String(
                    "SELECT title, score FROM catalog_pgadmin_docs".to_string()
                ),
            ]]
        );
        assert_eq!(
            table_data.rows,
            vec![vec![Value::String("alpha".to_string()), Value::Int64(9)]]
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
