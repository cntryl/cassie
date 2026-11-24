#[test]
fn should_keep_deadlock_victim_keys_until_transaction_resolution() {
    // Arrange
    let fixture = sql_fixture(
        "tx-unique-conflict-deadlock-savepoint",
        &["CREATE TABLE tx_unique_wait_deadlock (id INT PRIMARY KEY, label TEXT UNIQUE)"],
    );
    let cycle = start_deadlock_wait_cycle(&fixture);
    let before_cycle = cycle
        .result_receiver
        .recv_timeout(Duration::from_millis(500));
    assert!(
        matches!(before_cycle, Err(mpsc::RecvTimeoutError::Timeout)),
        "the first insert must be waiting on the second transaction before closing the cycle"
    );
    // Act
    let deadlock_result = fixture.cassie.execute_sql(
        &cycle.victim_session,
        "INSERT INTO tx_unique_wait_deadlock VALUES (4, 'first')",
        vec![],
    );
    let before_savepoint_rollback = cycle
        .result_receiver
        .recv_timeout(Duration::from_millis(500));
    let savepoint_result = fixture.cassie.execute_sql(
        &cycle.victim_session,
        "ROLLBACK TO SAVEPOINT after_key",
        vec![],
    );
    let (waiter_returned_before_savepoint_rollback, before_owner_commit) =
        resolve_waiter_after_savepoint(&cycle.result_receiver, before_savepoint_rollback);
    let waiter_returned_before_owner_commit = before_owner_commit.is_some();
    let owner_commit_result = fixture
        .cassie
        .execute_sql(&cycle.victim_session, "COMMIT", vec![]);
    if owner_commit_result.is_err() {
        fixture
            .cassie
            .execute_sql(&cycle.victim_session, "ROLLBACK", vec![])
            .expect("clean up the deadlock victim transaction");
    }
    let waiter_result = before_owner_commit.unwrap_or_else(|| {
        cycle
            .result_receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("waiting insert completes after owner transaction resolution")
    });
    cycle
        .waiter
        .join()
        .expect("waiting insert worker should finish");
    let first_resolution = if waiter_result.is_ok() {
        "COMMIT"
    } else {
        "ROLLBACK"
    };
    fixture
        .cassie
        .execute_sql(&cycle.first_session, first_resolution, vec![])
        .expect("resolve the first transaction after the waiting insert completes");
    // Assert
    assert!(
        cycle.request_entered_engine,
        "the first insert must enter the engine"
    );
    assert!(
        matches!(
            deadlock_result,
            Err(CassieError::Transaction {
                kind: TransactionErrorKind::DeadlockDetected,
                ..
            })
        ),
        "the second conflicting insert must detect the wait cycle"
    );
    assert!(
        !waiter_returned_before_savepoint_rollback,
        "deadlock detection must preserve keys owned before the failed statement"
    );
    assert!(
        savepoint_result.is_ok()
            && !waiter_returned_before_owner_commit
            && owner_commit_result.is_ok(),
        "savepoint recovery must keep the victim key until owner commit"
    );
    assert!(
        matches!(waiter_result, Err(CassieError::UniqueViolation { .. })),
        "the waiting INSERT fails after the recovered owner commits"
    );
    assert_eq!(
        fixture.rows("SELECT id FROM tx_unique_wait_deadlock ORDER BY id"),
        vec![vec![Value::Int64(2)]]
    );
}

