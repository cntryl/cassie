use cassie::app::{Cassie, CassieError};
use cassie::types::Value;

use super::support_sql as support;
use support::{data_dir, use_local_storage};

#[test]
fn should_reject_uuid_aliases_in_unique_constraint_reservations() {
    // Arrange
    use_local_storage();
    let path = data_dir("uuid_unique_aliases");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE uuid_unique_aliases (id TEXT, value UUID UNIQUE)",
            vec![],
        )
        .expect("create UUID unique table");
    cassie
            .execute_sql(
                &session,
                "INSERT INTO uuid_unique_aliases (id, value) VALUES ('first', 'a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11')",
                vec![],
            )
            .expect("insert canonical UUID");

    // Act
    let duplicate = cassie.execute_sql(
            &session,
            "INSERT INTO uuid_unique_aliases (id, value) VALUES ('alias', 'A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11')",
            vec![],
        );

    // Assert
    assert!(matches!(
        duplicate,
        Err(CassieError::UniqueViolation { .. })
    ));
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_bytea_aliases_in_unique_scalar_index_reservations() {
    // Arrange
    use_local_storage();
    let path = data_dir("bytea_unique_aliases");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE bytea_unique_aliases (id TEXT, value BYTEA)",
            vec![],
        )
        .expect("create BYTEA table");
    cassie
        .execute_sql(
            &session,
            "CREATE UNIQUE INDEX bytea_unique_aliases_value_idx ON bytea_unique_aliases (value)",
            vec![],
        )
        .expect("create BYTEA unique index");
    cassie
        .execute_sql(
            &session,
            r"INSERT INTO bytea_unique_aliases (id, value) VALUES ('first', '\xdeadbeef')",
            vec![],
        )
        .expect("insert canonical BYTEA");

    // Act
    let duplicate = cassie.execute_sql(
        &session,
        r"INSERT INTO bytea_unique_aliases (id, value) VALUES ('alias', '\xDEADBEEF')",
        vec![],
    );

    // Assert
    assert!(
        duplicate.is_err(),
        "unique scalar index must reject an alias"
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_find_uuid_alias_after_indexed_write() {
    // Arrange
    use_local_storage();
    let path = data_dir("uuid_scalar_index_alias");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE uuid_scalar_index_alias (id TEXT, value UUID)",
            vec![],
        )
        .expect("create UUID table");
    cassie
        .execute_sql(
            &session,
            "CREATE INDEX uuid_scalar_index_alias_value_idx ON uuid_scalar_index_alias (value)",
            vec![],
        )
        .expect("create UUID index");
    cassie
            .execute_sql(
                &session,
                "INSERT INTO uuid_scalar_index_alias (id, value) VALUES ('written', 'A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11')",
                vec![],
            )
            .expect("insert UUID alias");

    // Act
    let result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM uuid_scalar_index_alias WHERE value = 'a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11'",
                vec![],
            )
            .expect("query UUID index");

    // Assert
    assert_eq!(
        result.rows(),
        vec![vec![Value::String("written".to_string())]]
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_find_bytea_alias_after_indexed_write() {
    // Arrange
    use_local_storage();
    let path = data_dir("bytea_scalar_index_alias");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE bytea_scalar_index_alias (id TEXT, value BYTEA)",
            vec![],
        )
        .expect("create BYTEA table");
    cassie
        .execute_sql(
            &session,
            "CREATE INDEX bytea_scalar_index_alias_value_idx ON bytea_scalar_index_alias (value)",
            vec![],
        )
        .expect("create BYTEA index");
    cassie
        .execute_sql(
            &session,
            r"INSERT INTO bytea_scalar_index_alias (id, value) VALUES ('written', '\xDEADBEEF')",
            vec![],
        )
        .expect("insert BYTEA alias");

    // Act
    let result = cassie
        .execute_sql(
            &session,
            r"SELECT id FROM bytea_scalar_index_alias WHERE value = '\xdeadbeef'",
            vec![],
        )
        .expect("query BYTEA index");

    // Assert
    assert_eq!(
        result.rows(),
        vec![vec![Value::String("written".to_string())]]
    );
    let _ = std::fs::remove_dir_all(path);
}

fn deleted_owner_fixture(
    name: &str,
    column_ddl: &str,
    unique_index_ddl: Option<&str>,
    literal: &str,
) -> (Cassie, cassie::app::CassieSession, String) {
    use_local_storage();
    let path = data_dir(name);
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            &format!("CREATE TABLE {name} (id TEXT, value {column_ddl})"),
            vec![],
        )
        .expect("create table");
    if let Some(ddl) = unique_index_ddl {
        cassie
            .execute_sql(&session, ddl, vec![])
            .expect("create unique index");
    }
    cassie
        .execute_sql(
            &session,
            &format!("INSERT INTO {name} (id, value) VALUES ('first', '{literal}')"),
            vec![],
        )
        .expect("insert non-canonical value");
    cassie
        .execute_sql(
            &session,
            &format!("DELETE FROM {name} WHERE id = 'first'"),
            vec![],
        )
        .expect("delete owner");
    (cassie, session, path)
}

#[test]
fn should_release_uuid_unique_constraint_reservation_on_delete() {
    // Arrange
    let literal = "A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11";
    let (cassie, session, path) =
        deleted_owner_fixture("uuid_unique_delete_reuse", "UUID UNIQUE", None, literal);

    // Act
    let reused = cassie.execute_sql(
        &session,
        &format!("INSERT INTO uuid_unique_delete_reuse (id, value) VALUES ('second', '{literal}')"),
        vec![],
    );

    // Assert
    assert!(reused.is_ok(), "deleted value must be reusable");
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_release_bytea_unique_constraint_reservation_on_delete() {
    // Arrange
    let literal = r"\xDEADBEEF";
    let (cassie, session, path) =
        deleted_owner_fixture("bytea_unique_delete_reuse", "BYTEA UNIQUE", None, literal);

    // Act
    let reused = cassie.execute_sql(
        &session,
        &format!(
            "INSERT INTO bytea_unique_delete_reuse (id, value) VALUES ('second', '{literal}')"
        ),
        vec![],
    );

    // Assert
    assert!(reused.is_ok(), "deleted value must be reusable");
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_release_uuid_unique_index_reservation_on_delete() {
    // Arrange
    let literal = "A0EEBC99-9C0B-4EF8-BB6D-6BB9BD380A11";
    let (cassie, session, path) = deleted_owner_fixture(
            "uuid_unique_index_delete_reuse",
            "UUID",
            Some(
                "CREATE UNIQUE INDEX uuid_unique_index_delete_reuse_idx ON uuid_unique_index_delete_reuse (value)",
            ),
            literal,
        );

    // Act
    let reused = cassie.execute_sql(
        &session,
        &format!(
            "INSERT INTO uuid_unique_index_delete_reuse (id, value) VALUES ('second', '{literal}')"
        ),
        vec![],
    );

    // Assert
    assert!(reused.is_ok(), "deleted value must be reusable");
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_release_bytea_unique_index_reservation_on_delete() {
    // Arrange
    let literal = r"\xDEADBEEF";
    let (cassie, session, path) = deleted_owner_fixture(
            "bytea_unique_index_delete_reuse",
            "BYTEA",
            Some(
                "CREATE UNIQUE INDEX bytea_unique_index_delete_reuse_idx ON bytea_unique_index_delete_reuse (value)",
            ),
            literal,
        );

    // Act
    let reused = cassie.execute_sql(
            &session,
            &format!("INSERT INTO bytea_unique_index_delete_reuse (id, value) VALUES ('second', '{literal}')"),
            vec![],
        );

    // Assert
    assert!(reused.is_ok(), "deleted value must be reusable");
    let _ = std::fs::remove_dir_all(path);
}
