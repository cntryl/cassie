use super::support_sql as support;

use cassie::app::Cassie;
use cassie::types::Value;

use support::{data_dir, use_local_storage};

#[test]
fn should_not_widen_wildcard_rows_with_a_stray_id_payload_key() {
    // Arrange
    use_local_storage();
    let path = data_dir("reserved_id_stray_payload_key");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for statement in [
        "CREATE TABLE stray_id_rows (id INT, name TEXT)",
        "INSERT INTO stray_id_rows (id, name) VALUES (42, 'alice')",
        "ALTER TABLE stray_id_rows DROP COLUMN id",
    ] {
        cassie
            .execute_sql(&session, statement, vec![])
            .expect(statement);
    }

    // Act
    let selected = cassie
        .execute_sql(&session, "SELECT * FROM stray_id_rows", vec![])
        .expect("wildcard select");

    // Assert
    assert_eq!(
        selected.rows().len(),
        1,
        "expected the single stored document"
    );
    assert_eq!(
        selected.rows()[0].len(),
        selected.columns().len(),
        "row width must match the column list: columns {:?}, row {:?}",
        selected
            .columns()
            .iter()
            .map(|column| &column.name)
            .collect::<Vec<_>>(),
        selected.rows()[0]
    );
    // The dropped column's stored 42 must not resurface as the identity.
    assert_ne!(selected.rows()[0][0], Value::Int64(42));
    assert_eq!(selected.rows()[0][1], Value::String("alice".to_string()));
}
