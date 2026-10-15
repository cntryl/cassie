#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/sql_fixture.rs"]
mod support_sql_fixture;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;

use support_sql_fixture::sql_fixture;

include!("sql_relation_identifiers/contracts/restart.rs");

include!("sql_relation_identifiers/contracts/semantics.rs");

fn relation_fixture(label: &str, scoped: bool) -> support_sql_fixture::SqlFixture {
    let fixture = sql_fixture(label, &[]);
    if scoped {
        fixture.cassie.catalog.register_database("postgres", None);
    }
    fixture
        .execute("CREATE SCHEMA triage")
        .expect("create triage schema");
    fixture
        .execute("CREATE TABLE triage.dot (id BIGINT PRIMARY KEY)")
        .expect("create schema-qualified table");
    fixture
}

fn assert_quoted_and_qualified_relations_are_distinct(fixture: &support_sql_fixture::SqlFixture) {
    // Act
    let created = fixture
        .execute("CREATE TABLE \"triage.dot\" (id BIGINT PRIMARY KEY, other_id BIGINT UNIQUE)");
    let created_index =
        fixture.execute("CREATE INDEX dotted_relation_other_id_idx ON \"triage.dot\" (other_id)");
    let created_foreign_key = fixture.execute(
        "CREATE TABLE dotted_relation_child (id BIGINT PRIMARY KEY, relation_id BIGINT REFERENCES \"triage.dot\"(other_id))",
    );
    let inserted_dotted =
        fixture.execute("INSERT INTO \"triage.dot\" (id, other_id) VALUES (7, 70)");
    let inserted_dotted_for_delete =
        fixture.execute("INSERT INTO \"triage.dot\" (id, other_id) VALUES (8, 80)");
    let inserted_qualified = fixture.execute("INSERT INTO triage.dot (id) VALUES (9)");
    let updated_dotted = fixture.execute("UPDATE \"triage.dot\" SET other_id = 71 WHERE id = 7");
    let deleted_dotted = fixture.execute("DELETE FROM \"triage.dot\" WHERE id = 8");
    let dotted_rows = fixture.rows("SELECT other_id FROM \"triage.dot\"");
    let qualified_rows = fixture.rows("SELECT id FROM triage.dot");

    // Assert
    assert!(created.is_ok(), "CREATE TABLE should succeed");
    assert!(created_index.is_ok(), "CREATE INDEX should succeed");
    assert!(
        created_foreign_key.is_ok(),
        "foreign key target should succeed"
    );
    assert!(inserted_dotted.is_ok(), "quoted INSERT should succeed");
    assert!(
        inserted_qualified.is_ok(),
        "qualified INSERT should succeed"
    );
    assert!(
        inserted_dotted_for_delete.is_ok(),
        "quoted INSERT for DELETE should succeed"
    );
    assert!(updated_dotted.is_ok(), "quoted UPDATE should succeed");
    assert!(deleted_dotted.is_ok(), "quoted DELETE should succeed");
    assert_eq!(dotted_rows, vec![vec![cassie::types::Value::Int64(71)]]);
    assert_eq!(qualified_rows, vec![vec![cassie::types::Value::Int64(9)]]);
    let dotted_relation = fixture
        .cassie
        .midge
        .collection_metadata("postgres.public.\"triage.dot\"")
        .expect("read dotted relation metadata")
        .expect("dotted relation metadata");
    let qualified_relation = fixture
        .cassie
        .midge
        .collection_metadata("postgres.triage.dot")
        .expect("read qualified relation metadata")
        .expect("qualified relation metadata");
    assert_ne!(dotted_relation.storage_id, qualified_relation.storage_id);
}
