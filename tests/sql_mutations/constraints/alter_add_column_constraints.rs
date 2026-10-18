use cassie::app::Cassie;
use cassie::types::Value;

use super::support_sql as support;

#[test]
fn should_enforce_inline_constraints_added_to_existing_table() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("alter-add-column-inline-constraints");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("start Cassie");
    let session = cassie.create_session("tester", None);
    for sql in [
            "CREATE TABLE alter_add_column_parent (id INT PRIMARY KEY)",
            "INSERT INTO alter_add_column_parent (id) VALUES (1)",
            "CREATE TABLE alter_add_column_constraints (id INT PRIMARY KEY)",
            "ALTER TABLE alter_add_column_constraints ADD COLUMN required_value INT NOT NULL DEFAULT 5",
            "ALTER TABLE alter_add_column_constraints ADD COLUMN unique_value INT UNIQUE",
            "ALTER TABLE alter_add_column_constraints ADD COLUMN checked_value INT CHECK (checked_value > 0)",
            "ALTER TABLE alter_add_column_constraints ADD COLUMN referenced_value INT REFERENCES alter_add_column_parent(id)",
            "ALTER TABLE alter_add_column_constraints ADD COLUMN serial_value SERIAL",
        ] {
            cassie.execute_sql(&session, sql, vec![]).expect(sql);
        }

    // Act
    let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO alter_add_column_constraints (id, unique_value, checked_value, referenced_value) VALUES (1, 7, 1, 1)",
                vec![],
            )
            .expect("insert row omitting the defaulted column");
    let rows = cassie
        .execute_sql(
            &session,
            "SELECT required_value, serial_value FROM alter_add_column_constraints WHERE id = 1",
            vec![],
        )
        .expect("read defaulted and generated values")
        .rows()
        .to_vec();
    let null_not_null = cassie.execute_sql(
        &session,
        "INSERT INTO alter_add_column_constraints (id, required_value) VALUES (2, NULL)",
        vec![],
    );
    let duplicate_unique = cassie.execute_sql(
        &session,
        "INSERT INTO alter_add_column_constraints (id, unique_value) VALUES (3, 7)",
        vec![],
    );
    let check_violation = cassie.execute_sql(
        &session,
        "INSERT INTO alter_add_column_constraints (id, checked_value) VALUES (4, -1)",
        vec![],
    );
    let foreign_key_violation = cassie.execute_sql(
        &session,
        "INSERT INTO alter_add_column_constraints (id, referenced_value) VALUES (5, 999)",
        vec![],
    );
    let serial_sequence_exists = cassie
        .catalog
        .get_sequence(&cassie::catalog::serial_sequence_name(
            "alter_add_column_constraints",
            "serial_value",
        ))
        .is_some();
    drop(cassie);
    let _ = std::fs::remove_dir_all(path);

    // Assert
    assert_eq!(
        (
            inserted.command(),
            rows,
            null_not_null.is_err(),
            duplicate_unique.is_err(),
            check_violation.is_err(),
            foreign_key_violation.is_err(),
            serial_sequence_exists,
        ),
        (
            "INSERT 0 1",
            vec![vec![Value::Int64(5), Value::Int64(1)]],
            true,
            true,
            true,
            true,
            true,
        ),
        "ALTER TABLE ADD COLUMN must retain defaults, constraints, references, and serial behavior"
    );
}
