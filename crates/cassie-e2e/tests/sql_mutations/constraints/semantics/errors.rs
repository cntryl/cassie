#[test]
fn should_reject_altered_check_constraint_when_existing_row_violates_it() {
    // Arrange
    let path = data_dir("alter_check_existing_violation");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "CREATE TABLE check_existing (n INT)", vec![])
        .expect("create table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO check_existing (n) VALUES (-5)",
            vec![],
        )
        .expect("insert violating row");

    // Act
    let added = cassie.execute_sql(
        &session,
        "ALTER TABLE check_existing ADD CONSTRAINT positive_n CHECK (n > 0)",
        vec![],
    );

    // Assert
    assert!(added.is_err(), "CHECK was added over a violating row");
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_altered_primary_key_when_existing_row_has_null_key() {
    // Arrange
    let path = data_dir("alter_primary_key_existing_null");
    let cassie = crate::support_sql::new_in_memory_cassie().expect("create Cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, "CREATE TABLE primary_existing (id INT)", vec![])
        .expect("create table");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO primary_existing (id) VALUES (NULL)",
            vec![],
        )
        .expect("insert null key");

    // Act
    let added = cassie.execute_sql(
        &session,
        "ALTER TABLE primary_existing ADD CONSTRAINT primary_existing_pk PRIMARY KEY (id)",
        vec![],
    );

    // Assert
    assert!(added.is_err(), "PRIMARY KEY was added over a null key");
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_report_malformed_like_pattern_in_check_constraint() {
    // Arrange
    let path = data_dir("check_like_malformed_pattern");
    let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
    cassie.startup().unwrap();
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            r"CREATE TABLE check_like_pattern (code TEXT CHECK (code LIKE 'ab\'))",
            vec![],
        )
        .unwrap();

    // Act
    let inserted = cassie.execute_sql(
        &session,
        "INSERT INTO check_like_pattern (code) VALUES ('abc')",
        vec![],
    );

    // Assert
    let error = inserted.expect_err("malformed LIKE pattern must surface");
    assert!(
        error
            .to_string()
            .contains("LIKE pattern must not end with escape character"),
        "{error}"
    );
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_reject_insert_when_primary_key_is_duplicate() {
    // Arrange
    let path = data_dir("primary_key_duplicate");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE primary_key_duplicate (id INT PRIMARY KEY, title TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO primary_key_duplicate (id, title) VALUES (1, 'alpha')",
                vec![],
            )
            .unwrap();

        // Act
        let inserted = cassie.execute_sql(
            &session,
            "INSERT INTO primary_key_duplicate (id, title) VALUES (1, 'beta')",
            vec![],
        );

        // Assert
        assert!(inserted.is_err());
        assert!(inserted
            .unwrap_err()
            .to_string()
            .contains("unique constraint failed for 'id'"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_insert_when_primary_key_is_null() {
    // Arrange
    let path = data_dir("primary_key_null");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE primary_key_null (id INT PRIMARY KEY, title TEXT)",
                vec![],
            )
            .unwrap();

        // Act
        let inserted = cassie.execute_sql(
            &session,
            "INSERT INTO primary_key_null (id, title) VALUES (NULL, 'alpha')",
            vec![],
        );

        // Assert
        assert!(inserted.is_err());
        assert!(inserted.unwrap_err().to_string().contains("cannot be null"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_insert_when_unique_value_is_duplicate() {
    // Arrange
    let path = data_dir("unique_insert_duplicate");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE unique_insert_duplicate (email TEXT UNIQUE)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO unique_insert_duplicate (email) VALUES ('a@example.com')",
                vec![],
            )
            .unwrap();

        // Act
        let inserted = cassie.execute_sql(
            &session,
            "INSERT INTO unique_insert_duplicate (email) VALUES ('a@example.com')",
            vec![],
        );

        // Assert
        assert!(inserted.is_err());
        assert!(inserted
            .unwrap_err()
            .to_string()
            .contains("unique constraint failed for 'email'"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_update_when_unique_value_conflicts() {
    // Arrange
    let path = data_dir("unique_update_conflict");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE unique_update_conflict (email TEXT UNIQUE)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO unique_update_conflict (email) VALUES ('a@example.com')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO unique_update_conflict (email) VALUES ('b@example.com')",
                vec![],
            )
            .unwrap();

        // Act
        let updated = cassie
            .execute_sql(
                &session,
                "UPDATE unique_update_conflict SET email = 'a@example.com' WHERE email = 'b@example.com'",
                vec![],
            );

        // Assert
        assert!(updated.is_err());
        assert!(updated
            .unwrap_err()
            .to_string()
            .contains("unique constraint failed for 'email'"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_insert_when_unique_index_value_is_duplicate() {
    // Arrange
    let path = data_dir("unique_index_insert_duplicate");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE unique_index_insert_duplicate (email TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE UNIQUE INDEX unique_index_email_idx ON unique_index_insert_duplicate USING btree (email)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO unique_index_insert_duplicate (email) VALUES ('a@example.com')",
                vec![],
            )
            .unwrap();
        let collection = canonical_test_collection(&cassie, "unique_index_insert_duplicate");
        let index = cassie
            .catalog
            .get_index(&collection, "unique_index_email_idx")
            .expect("index metadata");

        // Act
        let inserted = cassie
            .execute_sql(
                &session,
                "INSERT INTO unique_index_insert_duplicate (email) VALUES ('a@example.com')",
                vec![],
            );

        // Assert
        assert!(matches!(
            inserted,
            Err(cassie::app::CassieError::UniqueViolation { constraint, .. })
                if constraint == cassie::catalog::local_name(&index.name)
        ));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_update_when_unique_index_value_conflicts() {
    // Arrange
    let path = data_dir("unique_index_update_conflict");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE unique_index_update_conflict (email TEXT)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "CREATE UNIQUE INDEX unique_index_update_email_idx ON unique_index_update_conflict USING btree (email)",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO unique_index_update_conflict (email) VALUES ('a@example.com')",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO unique_index_update_conflict (email) VALUES ('b@example.com')",
                vec![],
            )
            .unwrap();
        let collection = canonical_test_collection(&cassie, "unique_index_update_conflict");
        let index = cassie
            .catalog
            .get_index(&collection, "unique_index_update_email_idx")
            .expect("index metadata");

        // Act
        let updated = cassie
            .execute_sql(
                &session,
                "UPDATE unique_index_update_conflict SET email = 'a@example.com' WHERE email = 'b@example.com'",
                vec![],
            );

        // Assert
        assert!(matches!(
            updated,
            Err(cassie::app::CassieError::UniqueViolation { constraint, .. })
                if constraint == cassie::catalog::local_name(&index.name)
        ));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_insert_when_check_constraint_fails() {
    // Arrange
    let path = data_dir("check_insert_failure");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE check_insert_failure (score INT CHECK (score >= 18))",
                vec![],
            )
            .unwrap();

        // Act
        let inserted = cassie.execute_sql(
            &session,
            "INSERT INTO check_insert_failure (score) VALUES (17)",
            vec![],
        );

        // Assert
        assert!(inserted.is_err());
        assert!(inserted
            .unwrap_err()
            .to_string()
            .contains("check constraint failed"));

        let _ = std::fs::remove_dir_all(path);
    });
}

#[test]
fn should_reject_update_when_check_constraint_fails() {
    // Arrange
    let path = data_dir("check_update_failure");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");

    runtime.block_on(async {
        let cassie = crate::support_sql::new_in_memory_cassie().unwrap();
        cassie.startup().unwrap();
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE check_update_failure (score INT CHECK (score >= 18))",
                vec![],
            )
            .unwrap();
        cassie
            .execute_sql(
                &session,
                "INSERT INTO check_update_failure (score) VALUES (20)",
                vec![],
            )
            .unwrap();

        // Act
        let updated = cassie.execute_sql(
            &session,
            "UPDATE check_update_failure SET score = 17",
            vec![],
        );

        // Assert
        assert!(updated.is_err());
        let message = updated.unwrap_err().to_string();
        assert!(
            message.contains("check constraint failed"),
            "expected check constraint error, got {message}"
        );

        let _ = std::fs::remove_dir_all(path);
    });
}
