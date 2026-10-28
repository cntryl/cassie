#[test]
fn should_reject_ordinary_statements_in_a_failed_transaction_block() {
    // Arrange
    let fixture = fixture("tx-error-failed-block");
    fail_block(&fixture);

    // Act
    let errors = [
        "SELECT id FROM tx_error_items",
        "INSERT INTO tx_error_items (id, label) VALUES (2, 'ignored')",
        "SAVEPOINT late",
        "RELEASE SAVEPOINT late",
        "BEGIN",
    ]
    .map(|sql| transaction_error(&fixture.error(sql)));

    // Assert
    for error in errors {
        assert_eq!(
            error,
            Some((TransactionErrorKind::FailedTransaction, ABORTED.to_string()))
        );
    }
    assert_eq!(fixture.session.transaction_status(), "failed");
}

#[test]
fn should_roll_back_a_failed_transaction_block_on_commit() {
    // Arrange
    let fixture = fixture("tx-error-failed-commit");
    fail_block(&fixture);

    // Act
    let committed = fixture.execute("COMMIT");

    // Assert
    let Ok(committed) = committed else {
        panic!("COMMIT of a failed block must complete as ROLLBACK");
    };
    assert_eq!(committed.command(), "ROLLBACK");
    assert_eq!(fixture.session.transaction_status(), "idle");
    assert_eq!(committed_ids(&fixture), Vec::<Vec<Value>>::new());
}

#[test]
fn should_reject_missing_savepoints_as_invalid_savepoint() {
    // Arrange
    let fixture = fixture("tx-error-missing-savepoint");

    // Act
    let mut errors = Vec::new();
    for control in ["ROLLBACK TO SAVEPOINT missing", "RELEASE SAVEPOINT missing"] {
        assert!(fixture.execute("BEGIN").is_ok(), "BEGIN must open a block");
        errors.push(transaction_error(&fixture.error(control)));
        errors.push(transaction_error(&fixture.error("SELECT 1")));
        assert!(fixture.execute("ROLLBACK").is_ok(), "ROLLBACK must recover");
    }
    fail_block(&fixture);
    let failed_missing = transaction_error(&fixture.error("ROLLBACK TO SAVEPOINT missing"));
    let still_failed = fixture.session.transaction_status();

    // Assert
    let invalid = Some((
        TransactionErrorKind::InvalidSavepoint,
        "savepoint \"missing\" does not exist".to_string(),
    ));
    let aborted = Some((TransactionErrorKind::FailedTransaction, ABORTED.to_string()));
    assert_eq!(
        errors,
        vec![invalid.clone(), aborted.clone(), invalid.clone(), aborted]
    );
    assert_eq!(failed_missing, invalid);
    assert_eq!(still_failed, "failed");
}

#[test]
fn should_reject_savepoint_controls_outside_a_transaction_block() {
    // Arrange
    let fixture = fixture("tx-error-idle-savepoint");

    // Act
    let errors = [
        "SAVEPOINT idle_point",
        "ROLLBACK TO SAVEPOINT idle_point",
        "RELEASE SAVEPOINT idle_point",
    ]
    .map(|sql| transaction_error(&fixture.error(sql)));

    // Assert
    assert_eq!(
        errors,
        [
            "SAVEPOINT can only be used in transaction blocks",
            "ROLLBACK TO SAVEPOINT can only be used in transaction blocks",
            "RELEASE SAVEPOINT can only be used in transaction blocks",
        ]
        .map(|message| Some((
            TransactionErrorKind::NoActiveTransaction,
            message.to_string()
        )))
    );
    assert_eq!(fixture.session.transaction_status(), "idle");
}

#[test]
fn should_recover_a_failed_transaction_block_through_an_existing_savepoint() {
    // Arrange
    let fixture = fixture("tx-error-savepoint-recovery");
    for sql in [
        "BEGIN",
        "INSERT INTO tx_error_items (id, label) VALUES (1, 'kept')",
        "SAVEPOINT before_dup",
    ] {
        assert!(fixture.execute(sql).is_ok(), "setup failed: {sql}");
    }
    let duplicate = fixture.execute("INSERT INTO tx_error_items (id, label) VALUES (1, 'dup')");
    assert!(duplicate.is_err(), "duplicate key must fail the block");

    // Act
    let recovered = fixture.execute("ROLLBACK TO SAVEPOINT before_dup");
    let inserted = fixture.execute("INSERT INTO tx_error_items (id, label) VALUES (2, 'after')");
    let committed = fixture.execute("COMMIT");

    // Assert
    assert!(
        recovered.is_ok(),
        "existing savepoint must recover the block"
    );
    assert!(
        inserted.is_ok(),
        "recovered block must accept ordinary work"
    );
    let Ok(committed) = committed else {
        panic!("recovered block must commit");
    };
    assert_eq!(committed.command(), "COMMIT");
    assert_eq!(fixture.session.transaction_status(), "idle");
    assert_eq!(
        committed_ids(&fixture),
        vec![vec![Value::Int64(1)], vec![Value::Int64(2)]]
    );
}

#[test]
fn should_restore_session_settings_when_commit_rolls_back_a_failed_block() {
    // Arrange
    let fixture = fixture("tx-error-failed-commit-settings");
    let original = fixture.session.search_path();
    for sql in ["BEGIN", "SET search_path TO tx_error_other"] {
        assert!(fixture.execute(sql).is_ok(), "setup failed: {sql}");
    }
    let failed = fixture.execute("SELECT 1 / 0");
    assert!(failed.is_err(), "division by zero must fail the block");

    // Act
    let committed = fixture.execute("COMMIT");

    // Assert
    assert!(committed.is_ok(), "COMMIT of a failed block must complete");
    assert_eq!(fixture.session.search_path(), original);
    assert_eq!(fixture.session.transaction_status(), "idle");
}

