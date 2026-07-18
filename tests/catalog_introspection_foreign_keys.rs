use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;

#[path = "support/catalog.rs"]
mod support;
use support::{data_dir, execute_statement, query_rows, with_fallback};

struct NamedForeignKeyRows {
    table_constraints: Vec<Vec<Value>>,
    key_usage: Vec<Vec<Value>>,
    referential: Vec<Vec<Value>>,
    pg_constraint: Vec<Vec<Value>>,
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
