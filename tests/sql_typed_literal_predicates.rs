use cassie::app::Cassie;
use cassie::types::Value;

#[path = "support/sql.rs"]
mod support;
use support::{data_dir, use_local_storage};

fn cassie_for(label: &str) -> (Cassie, cassie::app::CassieSession, String) {
    use_local_storage();
    let path = data_dir(label);
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    (cassie, session, path)
}

#[test]
fn should_match_uuid_in_typed_literals() {
    // Arrange
    let (cassie, session, path) = cassie_for("uuid_in_literals");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE uuid_in_probe (item_id TEXT, item_uuid UUID)",
            vec![],
        )
        .expect("create UUID probe");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO uuid_in_probe VALUES ('row-1', '550e8400-e29b-41d4-a716-446655440000'), ('row-2', '550e8400-e29b-41d4-a716-446655440001')",
            vec![],
        )
        .expect("seed UUID probe");

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT item_id FROM uuid_in_probe WHERE item_uuid IN ('550e8400-e29b-41d4-a716-446655440001', '550e8400-e29b-41d4-a716-446655440002')",
            vec![],
        )
        .expect("execute UUID IN predicate");

    // Assert
    assert_eq!(result.rows, vec![vec![Value::String("row-2".to_string())]]);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_malformed_uuid_in_literals() {
    // Arrange
    let (cassie, session, path) = cassie_for("malformed_uuid_in_literals");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE malformed_uuid_in_probe (item_uuid UUID)",
            vec![],
        )
        .expect("create UUID probe");

    // Act
    let error = cassie
        .execute_sql(
            &session,
            "SELECT item_uuid FROM malformed_uuid_in_probe WHERE item_uuid IN ('not-a-uuid')",
            vec![],
        )
        .expect_err("reject malformed UUID IN literal");

    // Assert
    assert!(error.to_string().contains("invalid UUID literal"));
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_match_uuid_between_typed_literals() {
    // Arrange
    let (cassie, session, path) = cassie_for("uuid_between_literals");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE uuid_between_probe (item_id TEXT, item_uuid UUID)",
            vec![],
        )
        .expect("create UUID probe");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO uuid_between_probe VALUES ('before', '450e8400-e29b-41d4-a716-446655440000'), ('inside', '550e8400-e29b-41d4-a716-446655440001'), ('after', '650e8400-e29b-41d4-a716-446655440000')",
            vec![],
        )
        .expect("seed UUID probe");

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT item_id FROM uuid_between_probe WHERE item_uuid BETWEEN '550e8400-e29b-41d4-a716-446655440000' AND '550e8400-e29b-41d4-a716-446655440002'",
            vec![],
        )
        .expect("execute UUID BETWEEN predicate");

    // Assert
    assert_eq!(result.rows, vec![vec![Value::String("inside".to_string())]]);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_malformed_uuid_between_literals() {
    // Arrange
    let (cassie, session, path) = cassie_for("malformed_uuid_between_literals");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE malformed_uuid_between_probe (item_uuid UUID)",
            vec![],
        )
        .expect("create UUID probe");

    // Act
    let errors = [
        cassie
            .execute_sql(
                &session,
                "SELECT item_uuid FROM malformed_uuid_between_probe WHERE item_uuid BETWEEN 'not-a-uuid' AND '550e8400-e29b-41d4-a716-446655440002'",
                vec![],
            )
            .expect_err("reject malformed UUID lower bound"),
        cassie
            .execute_sql(
                &session,
                "SELECT item_uuid FROM malformed_uuid_between_probe WHERE item_uuid BETWEEN '550e8400-e29b-41d4-a716-446655440000' AND 'not-a-uuid'",
                vec![],
            )
            .expect_err("reject malformed UUID upper bound"),
    ];

    // Assert
    assert!(errors
        .iter()
        .all(|error| error.to_string().contains("invalid UUID literal")));
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_match_bytea_in_typed_literals() {
    // Arrange
    let (cassie, session, path) = cassie_for("bytea_in_literals");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE bytea_in_probe (item_id TEXT, payload BYTEA)",
            vec![],
        )
        .expect("create BYTEA probe");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO bytea_in_probe VALUES ('row-1', '\\x0102'), ('row-2', '\\x0304')",
            vec![],
        )
        .expect("seed BYTEA probe");

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT item_id FROM bytea_in_probe WHERE payload IN ('\\x0304', '\\x0506')",
            vec![],
        )
        .expect("execute BYTEA IN predicate");

    // Assert
    assert_eq!(result.rows, vec![vec![Value::String("row-2".to_string())]]);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_malformed_bytea_in_literals() {
    // Arrange
    let (cassie, session, path) = cassie_for("malformed_bytea_in_literals");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE malformed_bytea_in_probe (payload BYTEA)",
            vec![],
        )
        .expect("create BYTEA probe");

    // Act
    let error = cassie
        .execute_sql(
            &session,
            "SELECT payload FROM malformed_bytea_in_probe WHERE payload IN ('\\xzz')",
            vec![],
        )
        .expect_err("reject malformed BYTEA IN literal");

    // Assert
    assert!(error.to_string().contains("invalid BYTEA literal"));
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_match_bytea_between_typed_literals() {
    // Arrange
    let (cassie, session, path) = cassie_for("bytea_between_literals");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE bytea_between_probe (item_id TEXT, payload BYTEA)",
            vec![],
        )
        .expect("create BYTEA probe");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO bytea_between_probe VALUES ('inside', '\\x0180'), ('after', '\\x0200')",
            vec![],
        )
        .expect("seed BYTEA probe");

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT item_id FROM bytea_between_probe WHERE payload BETWEEN '\\x0100' AND '\\x01ff'",
            vec![],
        )
        .expect("execute BYTEA BETWEEN predicate");

    // Assert
    assert_eq!(result.rows, vec![vec![Value::String("inside".to_string())]]);
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_malformed_bytea_between_literals() {
    // Arrange
    let (cassie, session, path) = cassie_for("malformed_bytea_between_literals");
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE malformed_bytea_between_probe (payload BYTEA)",
            vec![],
        )
        .expect("create BYTEA probe");

    // Act
    let errors = [
        cassie
            .execute_sql(
                &session,
                "SELECT payload FROM malformed_bytea_between_probe WHERE payload BETWEEN '\\x0' AND '\\x01ff'",
                vec![],
            )
            .expect_err("reject malformed BYTEA lower bound"),
        cassie
            .execute_sql(
                &session,
                "SELECT payload FROM malformed_bytea_between_probe WHERE payload BETWEEN '\\x0100' AND 'not-bytea'",
                vec![],
            )
            .expect_err("reject malformed BYTEA upper bound"),
    ];

    // Assert
    assert!(errors
        .iter()
        .all(|error| error.to_string().contains("invalid BYTEA literal")));
    let _ = std::fs::remove_dir_all(path);
}
