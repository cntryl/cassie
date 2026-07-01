use cassie::app::{Cassie, CassieSession};
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

struct NamedForeignKeyRows {
    table_constraints: Vec<Vec<Value>>,
    key_usage: Vec<Vec<Value>>,
    referential: Vec<Vec<Value>>,
    pg_constraint: Vec<Vec<Value>>,
}

struct PgAdminCatalogRows {
    namespace: Vec<Vec<Value>>,
    classes: Vec<Vec<Value>>,
    columns: Vec<Vec<Value>>,
    defaults: Vec<Vec<Value>>,
    indexes: Vec<Vec<Value>>,
    constraints: Vec<Vec<Value>>,
    view: Vec<Vec<Value>>,
    table_data: Vec<Vec<Value>>,
}

fn execute_statement(cassie: &Cassie, session: &CassieSession, sql: &str) {
    cassie.execute_sql(session, sql, vec![]).unwrap();
}

fn query_rows(cassie: &Cassie, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
    cassie.execute_sql(session, sql, vec![]).unwrap().rows
}

fn apply_named_foreign_key_schema(cassie: &Cassie, session: &CassieSession) {
    for sql in [
        r#"CREATE TABLE "catalog_named_fk_parents" (
            "id" INT,
            CONSTRAINT "catalog_named_fk_parents_pkey" PRIMARY KEY ("id")
        )"#,
        r#"CREATE TABLE "catalog_named_fk_children" (
            "id" INT,
            "parent_id" INT,
            CONSTRAINT "catalog_named_fk_children_pkey" PRIMARY KEY ("id")
        )"#,
        r#"ALTER TABLE "catalog_named_fk_children"
            ADD CONSTRAINT "catalog_named_fk_children_parent_fkey"
            FOREIGN KEY ("parent_id") REFERENCES "catalog_named_fk_parents"("id")
            ON DELETE CASCADE ON UPDATE CASCADE"#,
    ] {
        execute_statement(cassie, session, sql);
    }
}

fn collect_named_foreign_key_rows(cassie: &Cassie, session: &CassieSession) -> NamedForeignKeyRows {
    NamedForeignKeyRows {
        table_constraints: query_rows(
            cassie,
            session,
            "SELECT constraint_name, constraint_type FROM information_schema.table_constraints WHERE table_name = 'catalog_named_fk_children' ORDER BY constraint_name",
        ),
        key_usage: query_rows(
            cassie,
            session,
            "SELECT constraint_name, column_name, ordinal_position FROM information_schema.key_column_usage WHERE table_name = 'catalog_named_fk_children' ORDER BY constraint_name",
        ),
        referential: query_rows(
            cassie,
            session,
            "SELECT constraint_name, unique_constraint_name, update_rule, delete_rule FROM information_schema.referential_constraints WHERE constraint_name = 'catalog_named_fk_children_parent_fkey'",
        ),
        pg_constraint: query_rows(
            cassie,
            session,
            "SELECT conname, contype FROM pg_catalog.pg_constraint WHERE conrelid = 'catalog_named_fk_children' ORDER BY conname",
        ),
    }
}

fn assert_named_foreign_key_rows(rows: &NamedForeignKeyRows) {
    assert_eq!(
        rows.table_constraints,
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
        rows.key_usage,
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
        rows.referential,
        vec![vec![
            Value::String("catalog_named_fk_children_parent_fkey".to_string()),
            Value::String("catalog_named_fk_parents_pkey".to_string()),
            Value::String("CASCADE".to_string()),
            Value::String("CASCADE".to_string())
        ]]
    );
    assert_eq!(
        rows.pg_constraint,
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
}

fn apply_pgadmin_catalog_fixture(cassie: &Cassie, session: &CassieSession) {
    for sql in [
        r#"CREATE TABLE "catalog_pgadmin_docs" (
            "id" INT DEFAULT 1,
            "title" VARCHAR(32) NOT NULL,
            "score" INT,
            CONSTRAINT "catalog_pgadmin_docs_pkey" PRIMARY KEY ("id"),
            CONSTRAINT "catalog_pgadmin_docs_score_check" CHECK (score >= 0)
        )"#,
        r#"CREATE INDEX "catalog_pgadmin_docs_title_idx"
            ON "catalog_pgadmin_docs" ("title")"#,
        "CREATE VIEW catalog_pgadmin_ready AS SELECT title, score FROM catalog_pgadmin_docs",
        "INSERT INTO catalog_pgadmin_docs (id, title, score) VALUES (1, 'alpha', 9)",
    ] {
        execute_statement(cassie, session, sql);
    }
}

fn collect_pgadmin_catalog_rows(cassie: &Cassie, session: &CassieSession) -> PgAdminCatalogRows {
    PgAdminCatalogRows {
        namespace: query_rows(
            cassie,
            session,
            "SELECT oid, nspname, pg_catalog.pg_get_userbyid(nspowner), pg_catalog.has_schema_privilege(nspname, 'USAGE') FROM pg_catalog.pg_namespace WHERE nspname = 'public'",
        ),
        classes: query_rows(
            cassie,
            session,
            "SELECT relname, relkind, relnamespace_oid, relhasindex, relpersistence, pg_catalog.quote_ident(relname), pg_catalog.has_table_privilege(relname, 'SELECT'), pg_catalog.pg_table_is_visible(oid) FROM pg_catalog.pg_class WHERE relname IN ('catalog_pgadmin_docs', 'catalog_pgadmin_docs_title_idx', 'catalog_pgadmin_ready') ORDER BY relname",
        ),
        columns: query_rows(
            cassie,
            session,
            "SELECT attname, attnum, pg_catalog.format_type(atttypid, atttypmod), attnotnull, atthasdef, attrelid_oid, attisdropped FROM pg_catalog.pg_attribute WHERE attrelid = 'catalog_pgadmin_docs' ORDER BY attnum",
        ),
        defaults: query_rows(
            cassie,
            session,
            "SELECT adsrc FROM pg_catalog.pg_attrdef WHERE adrelid = 'catalog_pgadmin_docs' ORDER BY adnum",
        ),
        indexes: query_rows(
            cassie,
            session,
            "SELECT indexrelid, indrelid, indexrelid_oid, indrelid_oid, indisunique, indisprimary, indisvalid FROM pg_catalog.pg_index WHERE indrelid = 'catalog_pgadmin_docs' ORDER BY indexrelid",
        ),
        constraints: query_rows(
            cassie,
            session,
            "SELECT conname, conrelid, conrelid_oid, contype, conkey, convalidated FROM pg_catalog.pg_constraint WHERE conrelid = 'catalog_pgadmin_docs' ORDER BY conname",
        ),
        view: query_rows(
            cassie,
            session,
            "SELECT table_name, view_definition FROM information_schema.views WHERE table_name = 'catalog_pgadmin_ready'",
        ),
        table_data: query_rows(
            cassie,
            session,
            "SELECT title, score FROM catalog_pgadmin_docs ORDER BY title",
        ),
    }
}

fn assert_pgadmin_namespace(rows: &PgAdminCatalogRows) -> i64 {
    assert_eq!(rows.namespace.len(), 1);
    assert_eq!(rows.namespace[0][1], Value::String("public".to_string()));
    assert_eq!(rows.namespace[0][2], Value::String("postgres".to_string()));
    assert_eq!(rows.namespace[0][3], Value::Bool(true));
    let Value::Int64(public_oid) = rows.namespace[0][0] else {
        panic!("public namespace oid should be numeric");
    };
    public_oid
}

fn assert_pgadmin_classes(rows: &PgAdminCatalogRows, public_oid: i64) {
    assert_eq!(
        rows.classes,
        vec![
            pgadmin_class_row("catalog_pgadmin_docs", "r", true, public_oid),
            pgadmin_class_row("catalog_pgadmin_docs_title_idx", "i", false, public_oid),
            pgadmin_class_row("catalog_pgadmin_ready", "v", false, public_oid),
        ]
    );
}

fn pgadmin_class_row(name: &str, relkind: &str, has_index: bool, public_oid: i64) -> Vec<Value> {
    vec![
        Value::String(name.to_string()),
        Value::String(relkind.to_string()),
        Value::Int64(public_oid),
        Value::Bool(has_index),
        Value::String("p".to_string()),
        Value::String(name.to_string()),
        Value::Bool(true),
        Value::Bool(true),
    ]
}

fn assert_pgadmin_columns(rows: &PgAdminCatalogRows) -> i64 {
    let Value::Int64(table_oid) = rows.columns[0][5] else {
        panic!("table oid should be numeric");
    };
    assert!(table_oid > 0);
    assert_eq!(
        rows.columns,
        vec![
            pgadmin_column_row("id", 1, "integer", true, true, table_oid),
            pgadmin_column_row("title", 2, "character varying(32)", true, false, table_oid),
            pgadmin_column_row("score", 3, "integer", false, false, table_oid),
        ]
    );
    table_oid
}

fn pgadmin_column_row(
    name: &str,
    attnum: i64,
    data_type: &str,
    not_null: bool,
    has_default: bool,
    table_oid: i64,
) -> Vec<Value> {
    vec![
        Value::String(name.to_string()),
        Value::Int64(attnum),
        Value::String(data_type.to_string()),
        Value::Bool(not_null),
        Value::Bool(has_default),
        Value::Int64(table_oid),
        Value::Bool(false),
    ]
}

fn assert_pgadmin_indexes(rows: &PgAdminCatalogRows, table_oid: i64) {
    assert_eq!(rows.defaults, vec![vec![Value::String("1".to_string())]]);
    assert_eq!(rows.indexes.len(), 2);
    assert_eq!(
        rows.indexes
            .iter()
            .map(|row| row[0].clone())
            .collect::<Vec<_>>(),
        vec![
            Value::String("catalog_pgadmin_docs_pkey".to_string()),
            Value::String("catalog_pgadmin_docs_title_idx".to_string()),
        ]
    );
    assert!(rows.indexes.iter().all(|row| row[2].as_i64().is_some()));
    assert!(rows
        .indexes
        .iter()
        .all(|row| row[3] == Value::Int64(table_oid)));
    assert!(rows.indexes.iter().all(|row| row[6] == Value::Bool(true)));
}

fn assert_pgadmin_constraints(rows: &PgAdminCatalogRows, table_oid: i64) {
    assert_eq!(
        rows.constraints,
        vec![
            pgadmin_constraint_row("catalog_pgadmin_docs_id_n", "n", "1", table_oid),
            pgadmin_constraint_row("catalog_pgadmin_docs_pkey", "p", "1", table_oid),
            pgadmin_constraint_row("catalog_pgadmin_docs_score_check", "c", "3", table_oid),
            pgadmin_constraint_row("catalog_pgadmin_docs_title_n", "n", "2", table_oid),
        ]
    );
}

fn pgadmin_constraint_row(name: &str, kind: &str, key: &str, table_oid: i64) -> Vec<Value> {
    vec![
        Value::String(name.to_string()),
        Value::String("catalog_pgadmin_docs".to_string()),
        Value::Int64(table_oid),
        Value::String(kind.to_string()),
        Value::String(key.to_string()),
        Value::Bool(true),
    ]
}

fn assert_pgadmin_view_and_data(rows: &PgAdminCatalogRows) {
    assert_eq!(
        rows.view,
        vec![vec![
            Value::String("catalog_pgadmin_ready".to_string()),
            Value::String("SELECT title, score FROM catalog_pgadmin_docs".to_string()),
        ]]
    );
    assert_eq!(
        rows.table_data,
        vec![vec![Value::String("alpha".to_string()), Value::Int64(9)]]
    );
}

fn assert_pgadmin_catalog_rows(rows: &PgAdminCatalogRows) {
    let public_oid = assert_pgadmin_namespace(rows);
    assert_pgadmin_classes(rows, public_oid);
    let table_oid = assert_pgadmin_columns(rows);
    assert_pgadmin_indexes(rows, table_oid);
    assert_pgadmin_constraints(rows, table_oid);
    assert_pgadmin_view_and_data(rows);
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
        let config = CassieRuntimeConfig {
            user: "postgres".to_string(),
            ..CassieRuntimeConfig::default()
        };

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
        apply_named_foreign_key_schema(&cassie, &session);

        // Act
        let rows = collect_named_foreign_key_rows(&cassie, &session);

        // Assert
        assert_named_foreign_key_rows(&rows);

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
        apply_pgadmin_catalog_fixture(&cassie, &session);

        // Act
        let rows = collect_pgadmin_catalog_rows(&cassie, &session);

        // Assert
        assert_pgadmin_catalog_rows(&rows);

        let _ = std::fs::remove_dir_all(path);
    });
}
