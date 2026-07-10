use super::{data_dir, execute_statement, query_rows, with_fallback};
use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;

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
        let session = cassie.create_session("tester", Some("postgres".to_string()));
        apply_pgadmin_catalog_fixture(&cassie, &session);

        // Act
        let rows = collect_pgadmin_catalog_rows(&cassie, &session);

        // Assert
        assert_pgadmin_catalog_rows(&rows);

        let _ = std::fs::remove_dir_all(path);
    });
}
