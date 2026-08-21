use cassie::app::Cassie;
use cassie::types::Value;

#[path = "support/sql.rs"]
mod support;
use support::{data_dir, use_local_storage};

#[test]
fn should_canonicalize_typed_predicate_literals() {
    // Arrange
    use_local_storage();
    let path = data_dir("canonical_typed_literals");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE canonical_typed_probe (item_id TEXT, item_uuid UUID, payload BYTEA)",
            vec![],
        )
        .expect("create typed probe");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO canonical_typed_probe VALUES ('MixedCase', '550e8400-e29b-41d4-a716-446655440000', '\\x0a0b')",
            vec![],
        )
        .expect("seed typed probe");

    // Act
    let predicates = [
        "item_uuid = '550E8400E29B41D4A716446655440000'",
        "'550E8400E29B41D4A716446655440000' = item_uuid",
        "item_uuid IN ('550E8400E29B41D4A716446655440000')",
        "item_uuid BETWEEN '550E8400E29B41D4A716446655440000' AND '550E8400E29B41D4A716446655440000'",
        "payload = '\\x0A0B'",
        "'\\x0A0B' = payload",
        "payload IN ('\\x0A0B')",
        "payload BETWEEN '\\x0A0B' AND '\\x0A0B'",
    ];
    let results = predicates.map(|predicate| {
        cassie
            .execute_sql(
                &session,
                &format!("SELECT item_id FROM canonical_typed_probe WHERE {predicate}"),
                vec![],
            )
            .expect("execute canonical typed predicate")
    });
    let text_control = cassie
        .execute_sql(
            &session,
            "SELECT item_id FROM canonical_typed_probe WHERE item_id = 'mixedcase'",
            vec![],
        )
        .expect("execute text control");

    // Assert
    for result in results {
        assert_eq!(
            result.rows,
            vec![vec![Value::String("MixedCase".to_string())]]
        );
    }
    assert!(text_control.rows.is_empty());
    let _ = std::fs::remove_dir_all(path);
}
