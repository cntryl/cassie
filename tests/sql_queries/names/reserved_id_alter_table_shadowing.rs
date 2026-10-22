use super::support_sql as support;

use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;

use support::data_dir;

fn run(cassie: &Cassie, session: &CassieSession, statements: &[&str]) {
    for statement in statements {
        cassie
            .execute_sql(session, statement, vec![])
            .expect(statement);
    }
}

#[test]
fn should_shadow_the_identity_alias_after_adding_an_id_column() {
    // Arrange
    let path = data_dir("reserved_id_alter_add_id_column");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    run(
        &cassie,
        &session,
        &[
            "CREATE TABLE alter_shadow_docs (name TEXT)",
            "INSERT INTO alter_shadow_docs (name) VALUES ('alice')",
        ],
    );
    let identity = cassie
        .execute_sql(&session, "SELECT id FROM alter_shadow_docs", vec![])
        .expect("bare id before the alter")
        .rows()[0][0]
        .clone();
    assert!(
        matches!(&identity, Value::String(value) if !value.is_empty()),
        "expected the internal identity before the alter, got {identity:?}"
    );

    // Act
    run(
        &cassie,
        &session,
        &["ALTER TABLE alter_shadow_docs ADD COLUMN id INT"],
    );
    let shadowed = cassie
        .execute_sql(
            &session,
            "SELECT id, _id, name FROM alter_shadow_docs",
            vec![],
        )
        .expect("bare id after the alter");
    run(
        &cassie,
        &session,
        &["ALTER TABLE alter_shadow_docs DROP COLUMN id"],
    );
    let restored = cassie
        .execute_sql(&session, "SELECT id FROM alter_shadow_docs", vec![])
        .expect("bare id after dropping the column");

    // Assert
    assert_eq!(shadowed.rows()[0][0], Value::Null);
    assert_eq!(shadowed.rows()[0][1], identity.clone());
    assert_eq!(shadowed.rows()[0][2], Value::String("alice".to_string()));
    assert_eq!(restored.rows(), vec![vec![identity]]);

    let _ = std::fs::remove_dir_all(path);
}
