use super::support_sql as support;

use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;

use support::{data_dir, use_local_storage};

fn run(cassie: &Cassie, session: &CassieSession, statements: &[&str]) {
    for statement in statements {
        cassie
            .execute_sql(session, statement, vec![])
            .expect(statement);
    }
}

#[test]
fn should_return_declared_id_when_column_index_covers_only_other_fields() {
    // Arrange
    use_local_storage();
    let path = data_dir("column_index_declared_id_projection");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    run(
        &cassie,
        &session,
        &[
            "CREATE TABLE declared_id_labels (id INT, label TEXT)",
            "INSERT INTO declared_id_labels (id, label) VALUES (42, 'q')",
            "INSERT INTO declared_id_labels (id, label) VALUES (7, 'r')",
            "CREATE INDEX declared_id_labels_col ON declared_id_labels USING column (label)",
        ],
    );

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT id, label FROM declared_id_labels WHERE label = 'q'",
            vec![],
        )
        .expect("select declared id through column index");

    // Assert
    assert_eq!(
        result.rows(),
        vec![vec![Value::Int64(42), Value::String("q".to_string())]]
    );

    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_filter_on_declared_id_through_column_index() {
    // Arrange
    use_local_storage();
    let path = data_dir("column_index_declared_id_filter");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    run(
        &cassie,
        &session,
        &[
            "CREATE TABLE declared_id_filter (id INT, label TEXT)",
            "INSERT INTO declared_id_filter (id, label) VALUES (42, 'q')",
            "INSERT INTO declared_id_filter (id, label) VALUES (7, 'r')",
            "CREATE INDEX declared_id_filter_col ON declared_id_filter USING column (id, label)",
        ],
    );

    // Act
    let result = cassie
        .execute_sql(
            &session,
            "SELECT label FROM declared_id_filter WHERE id = 42",
            vec![],
        )
        .expect("filter declared id through column index");

    // Assert
    assert_eq!(result.rows(), vec![vec![Value::String("q".to_string())]]);

    let _ = std::fs::remove_dir_all(path);
}
