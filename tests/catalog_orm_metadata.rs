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
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                r#"CREATE TABLE "catalog_orm_parents" (
                    "id" INT NOT NULL DEFAULT 7,
                    "code" VARCHAR(16) NOT NULL,
                    "label" TEXT DEFAULT 'new',
                    CONSTRAINT "catalog_orm_parents_pkey" PRIMARY KEY ("id"),
                    CONSTRAINT "catalog_orm_parents_code_key" UNIQUE ("code"),
                    CONSTRAINT "catalog_orm_parents_id_check" CHECK (id > 0)
                )"#,
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                r#"CREATE TABLE "catalog_orm_children" (
                    "id" INT NOT NULL,
                    "parent_id" INT DEFAULT 7,
                    CONSTRAINT "catalog_orm_children_pkey" PRIMARY KEY ("id"),
                    CONSTRAINT "catalog_orm_children_parent_fkey"
                        FOREIGN KEY ("parent_id") REFERENCES "catalog_orm_parents"("id")
                        ON DELETE SET NULL ON UPDATE CASCADE
                )"#,
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                r#"CREATE INDEX "catalog_orm_children_parent_idx"
                    ON "catalog_orm_children" ("parent_id")"#,
                vec![],
            )
            .unwrap();

        // Act
        let columns = cassie
            .execute_sql(
                &session,
                "SELECT column_name, ordinal_position, is_nullable, data_type, udt_name, column_default, character_maximum_length, numeric_precision, numeric_scale, datetime_precision FROM information_schema.columns WHERE table_name = 'catalog_orm_parents' ORDER BY ordinal_position",
                vec![],
            )
            .unwrap();
        let attributes = cassie
            .execute_sql(
                &session,
                "SELECT attname, attnum, atttypid, attnotnull, atttypmod, atthasdef FROM pg_catalog.pg_attribute WHERE attrelid = 'catalog_orm_parents' ORDER BY attnum",
                vec![],
            )
            .unwrap();
        let defaults = cassie
            .execute_sql(
                &session,
                "SELECT adrelid, adnum, adsrc FROM pg_catalog.pg_attrdef WHERE adrelid = 'catalog_orm_parents' ORDER BY adnum",
                vec![],
            )
            .unwrap();
        let indexes = cassie
            .execute_sql(
                &session,
                "SELECT indexrelid, indrelid, indisunique, indisprimary, indkey FROM pg_catalog.pg_index WHERE indrelid = 'catalog_orm_children' ORDER BY indexrelid",
                vec![],
            )
            .unwrap();

        // Assert
        assert_eq!(
            columns.rows,
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
        assert_eq!(
            attributes.rows,
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
        assert_eq!(
            defaults.rows,
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
        assert_eq!(
            indexes.rows,
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

        let _ = std::fs::remove_dir_all(path);
    });
}
