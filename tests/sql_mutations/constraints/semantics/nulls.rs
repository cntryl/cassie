#[test]
fn should_satisfy_check_constraints_when_the_value_is_explicit_null() {
    // Arrange
    let path = data_dir("check_explicit_null");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    for sql in [
        "CREATE TABLE check_nulls (age INT CHECK (age > 0), code TEXT CHECK (code = 'x'), v INT)",
        "INSERT INTO check_nulls (age, code, v) VALUES (5, 'x', 1)",
        "CREATE TABLE check_null_existing (n INT, v INT)",
        "INSERT INTO check_null_existing (n, v) VALUES (NULL, 1)",
    ] {
        cassie.execute_sql(&session, sql, vec![]).expect(sql);
    }

    // Act
    let inserted = cassie.execute_sql(
        &session,
        "INSERT INTO check_nulls (age, code, v) VALUES (NULL, NULL, 2)",
        vec![],
    );
    let updated = cassie.execute_sql(
        &session,
        "UPDATE check_nulls SET age = NULL, code = NULL WHERE v = 1",
        vec![],
    );
    let added = cassie.execute_sql(
        &session,
        "ALTER TABLE check_null_existing ADD CONSTRAINT positive_n CHECK (n > 0)",
        vec![],
    );
    let stored = cassie
        .execute_sql(
            &session,
            "SELECT age, code, v FROM check_nulls ORDER BY v",
            vec![],
        )
        .expect("select rows")
        .rows()
        .to_vec();

    // Assert
    assert!(inserted.is_ok(), "explicit NULL must satisfy the CHECKs");
    assert!(updated.is_ok(), "updating to NULL must satisfy the CHECKs");
    assert!(added.is_ok(), "an existing NULL must satisfy a new CHECK");
    assert_eq!(
        stored,
        vec![
            vec![Value::Null, Value::Null, Value::Int64(1)],
            vec![Value::Null, Value::Null, Value::Int64(2)],
        ]
    );
    let _ = std::fs::remove_dir_all(path);
}
