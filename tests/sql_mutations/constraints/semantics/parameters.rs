#[test]
fn should_bind_altered_check_constraint_to_declared_column_case() {
    // Arrange
    use_local_storage();
    let path = data_dir("alter_constraint_column_case");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "CREATE TABLE check_case (Age INT, v INT)", vec![])
        .expect("create check table");
    cassie
        .execute_sql(
            &session,
            "ALTER TABLE check_case ADD CONSTRAINT age_positive CHECK (age > 0)",
            vec![],
        )
        .expect("add check constraint");
    // Act
    let invalid_check = cassie.execute_sql(
        &session,
        "INSERT INTO check_case (Age, v) VALUES (-5, 1)",
        vec![],
    );

    // Assert
    assert!(invalid_check.is_err(), "case-mismatched CHECK was skipped");
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_bind_altered_unique_constraint_to_declared_column_case() {
    // Arrange
    use_local_storage();
    let path = data_dir("alter_unique_constraint_column_case");
    let cassie = Cassie::new_with_data_dir(&path).expect("create Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE unique_case (Age INT, v INT)",
            vec![],
        )
        .expect("create unique table");
    cassie
        .execute_sql(
            &session,
            "ALTER TABLE unique_case ADD CONSTRAINT age_unique UNIQUE (age)",
            vec![],
        )
        .expect("add unique constraint");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO unique_case (Age, v) VALUES (5, 1)",
            vec![],
        )
        .expect("insert first unique row");

    // Act
    let duplicate_unique = cassie.execute_sql(
        &session,
        "INSERT INTO unique_case (Age, v) VALUES (5, 2)",
        vec![],
    );

    // Assert
    assert!(
        duplicate_unique.is_err(),
        "case-mismatched UNIQUE was skipped"
    );
    let _ = std::fs::remove_dir_all(path);
}
