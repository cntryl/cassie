use cassie::app::Cassie;
use cassie::types::Value;
use std::path::PathBuf;
use uuid::Uuid;

fn with_fallback() {
    if std::env::var("CASSIE_EMBEDDINGS_PROVIDER").is_err() {
        std::env::set_var("CASSIE_EMBEDDINGS_PROVIDER", "fallback");
    }
}

fn data_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("cassie-catalog-orm-{name}-{}", Uuid::new_v4()))
}

struct OrmMetadataRows {
    columns: Vec<Vec<Value>>,
    attributes: Vec<Vec<Value>>,
    defaults: Vec<Vec<Value>>,
    indexes: Vec<Vec<Value>>,
}

fn assert_orm_columns(rows: &OrmMetadataRows) {
    assert_eq!(
        rows.columns,
        vec![
            vec![
                Value::String("id".to_string()),
                Value::Int64(1),
                Value::String("NO".to_string()),
                Value::String("int".to_string()),
                Value::String("int4".to_string()),
                Value::String("7".to_string()),
                Value::Null,
                Value::Int64(32),
                Value::Int64(0),
                Value::Null,
            ],
            vec![
                Value::String("code".to_string()),
                Value::Int64(2),
                Value::String("NO".to_string()),
                Value::String("varchar(16)".to_string()),
                Value::String("varchar".to_string()),
                Value::Null,
                Value::Int64(16),
                Value::Null,
                Value::Null,
                Value::Null,
            ],
            vec![
                Value::String("label".to_string()),
                Value::Int64(3),
                Value::String("YES".to_string()),
                Value::String("text".to_string()),
                Value::String("text".to_string()),
                Value::String("'new'".to_string()),
                Value::Null,
                Value::Null,
                Value::Null,
                Value::Null,
            ],
        ]
    );
}

fn assert_orm_attributes(rows: &OrmMetadataRows) {
    assert_eq!(
        rows.attributes,
        vec![
            vec![
                Value::String("id".to_string()),
                Value::Int64(1),
                Value::Int64(23),
                Value::Bool(true),
                Value::Int64(-1),
                Value::Bool(true),
            ],
            vec![
                Value::String("code".to_string()),
                Value::Int64(2),
                Value::Int64(1043),
                Value::Bool(true),
                Value::Int64(20),
                Value::Bool(false),
            ],
            vec![
                Value::String("label".to_string()),
                Value::Int64(3),
                Value::Int64(25),
                Value::Bool(false),
                Value::Int64(-1),
                Value::Bool(true),
            ],
        ]
    );
}

fn assert_orm_defaults(rows: &OrmMetadataRows) {
    assert_eq!(
        rows.defaults,
        vec![
            vec![
                Value::String("catalog_orm_parents".to_string()),
                Value::Int64(1),
                Value::String("7".to_string()),
            ],
            vec![
                Value::String("catalog_orm_parents".to_string()),
                Value::Int64(3),
                Value::String("'new'".to_string()),
            ],
        ]
    );
}

fn assert_orm_indexes(rows: &OrmMetadataRows) {
    assert_eq!(
        rows.indexes,
        vec![
            vec![
                Value::String("catalog_orm_children_parent_idx".to_string()),
                Value::String("catalog_orm_children".to_string()),
                Value::Bool(false),
                Value::Bool(false),
                Value::String("2".to_string()),
            ],
            vec![
                Value::String("catalog_orm_children_pkey".to_string()),
                Value::String("catalog_orm_children".to_string()),
                Value::Bool(true),
                Value::Bool(true),
                Value::String("1".to_string()),
            ],
        ]
    );
}

fn apply_orm_metadata_fixture(cassie: &Cassie) {
    let session = cassie.create_session("tester", None);
    for sql in [
        r#"CREATE TABLE "catalog_orm_parents" (
            "id" INT NOT NULL DEFAULT 7,
            "code" VARCHAR(16) NOT NULL,
            "label" TEXT DEFAULT 'new',
            CONSTRAINT "catalog_orm_parents_pkey" PRIMARY KEY ("id"),
            CONSTRAINT "catalog_orm_parents_code_key" UNIQUE ("code"),
            CONSTRAINT "catalog_orm_parents_id_check" CHECK (id > 0)
        )"#,
        r#"CREATE TABLE "catalog_orm_children" (
            "id" INT NOT NULL,
            "parent_id" INT DEFAULT 7,
            CONSTRAINT "catalog_orm_children_pkey" PRIMARY KEY ("id"),
            CONSTRAINT "catalog_orm_children_parent_fkey"
                FOREIGN KEY ("parent_id") REFERENCES "catalog_orm_parents"("id")
                ON DELETE SET NULL ON UPDATE CASCADE
        )"#,
        r#"CREATE INDEX "catalog_orm_children_parent_idx"
            ON "catalog_orm_children" ("parent_id")"#,
    ] {
        cassie.execute_sql(&session, sql, vec![]).unwrap();
    }
}

fn query_rows(cassie: &Cassie, sql: &str) -> Vec<Vec<Value>> {
    let session = cassie.create_session("tester", None);
    cassie.execute_sql(&session, sql, vec![]).unwrap().rows
}

fn collect_orm_metadata_rows(cassie: &Cassie) -> OrmMetadataRows {
    OrmMetadataRows {
        columns: query_rows(
            cassie,
            "SELECT column_name, ordinal_position, is_nullable, data_type, udt_name, column_default, character_maximum_length, numeric_precision, numeric_scale, datetime_precision FROM information_schema.columns WHERE table_name = 'catalog_orm_parents' ORDER BY ordinal_position",
        ),
        attributes: query_rows(
            cassie,
            "SELECT attname, attnum, atttypid, attnotnull, atttypmod, atthasdef FROM pg_catalog.pg_attribute WHERE attrelid = 'catalog_orm_parents' ORDER BY attnum",
        ),
        defaults: query_rows(
            cassie,
            "SELECT adrelid, adnum, adsrc FROM pg_catalog.pg_attrdef WHERE adrelid = 'catalog_orm_parents' ORDER BY adnum",
        ),
        indexes: query_rows(
            cassie,
            "SELECT indexrelid, indrelid, indisunique, indisprimary, indkey FROM pg_catalog.pg_index WHERE indrelid = 'catalog_orm_children' ORDER BY indexrelid",
        ),
    }
}

fn assert_orm_metadata_rows(rows: &OrmMetadataRows) {
    assert_orm_columns(rows);
    assert_orm_attributes(rows);
    assert_orm_defaults(rows);
    assert_orm_indexes(rows);
}

#[test]
fn should_expose_orm_introspection_metadata_through_catalog_views() {
    // Arrange
    with_fallback();
    let path = data_dir("metadata");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).unwrap();
        cassie.startup().unwrap();
        apply_orm_metadata_fixture(&cassie);

        // Act
        let rows = collect_orm_metadata_rows(&cassie);

        // Assert
        assert_orm_metadata_rows(&rows);

        let _ = std::fs::remove_dir_all(path);
    });
}
