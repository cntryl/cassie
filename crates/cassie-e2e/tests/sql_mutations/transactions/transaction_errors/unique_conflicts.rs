#[test]
fn should_wait_for_a_unique_conflict_owner_before_returning_insert() {
    // Arrange
    let fixture = sql_fixture(
        "tx-unique-conflict-wait",
        &[
            "CREATE TABLE tx_unique_wait (id SERIAL PRIMARY KEY, label TEXT UNIQUE)",
            "BEGIN",
            "INSERT INTO tx_unique_wait (label) VALUES ('same') RETURNING id",
        ],
    );
    let owner_session = fixture.session.clone();
    let waiter_session = fixture.cassie.create_session("waiter", None);
    let cassie = fixture.cassie.clone();
    fixture
        .cassie
        .execute_sql(&waiter_session, "BEGIN", vec![])
        .expect("begin the conflicting transaction");

    let (result_sender, result_receiver) = mpsc::sync_channel(1);
    let started = Arc::new(Barrier::new(2));
    let thread_started = Arc::clone(&started);
    let thread_session = waiter_session.clone();
    let waiter = std::thread::spawn(move || {
        thread_started.wait();
        let result = cassie.execute_sql(
            &thread_session,
            "INSERT INTO tx_unique_wait (label) VALUES ('same') RETURNING id",
            vec![],
        );
        result_sender
            .send(result)
            .expect("main test thread receives the insert result");
    });
    started.wait();
    let request_entered_engine = wait_for_running_query(&fixture.cassie);
    let before_owner_commit = result_receiver.recv_timeout(Duration::from_millis(500));
    let returned_before_owner_commit = before_owner_commit.is_ok();

    // Act
    fixture
        .cassie
        .execute_sql(&owner_session, "COMMIT", vec![])
        .expect("resolve the unique-key owner transaction");
    let waiter_result = match before_owner_commit {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => result_receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("conflicting insert completes after owner resolution"),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            panic!("conflicting insert worker exited without returning a result")
        }
    };
    waiter
        .join()
        .expect("conflicting insert worker should finish");
    let waiter_state = waiter_session.transaction_status();
    fixture
        .cassie
        .execute_sql(&waiter_session, "ROLLBACK", vec![])
        .expect("recover the failed conflicting transaction");

    // Assert
    assert!(
        request_entered_engine,
        "the conflicting INSERT must enter the engine before owner commit"
    );
    assert!(
        !returned_before_owner_commit,
        "PostgreSQL waits for the unresolved unique-key owner before returning INSERT"
    );
    assert!(
        matches!(waiter_result, Err(CassieError::UniqueViolation { .. })),
        "committing the owner first makes the waiting INSERT fail with 23505"
    );
    assert_eq!(waiter_state, "failed");
    assert_eq!(
        fixture.rows("SELECT id FROM tx_unique_wait ORDER BY id"),
        vec![vec![Value::Int64(1)]]
    );
    assert_eq!(
        fixture.rows("INSERT INTO tx_unique_wait (label) VALUES ('next') RETURNING id"),
        vec![vec![Value::Int64(3)]],
        "the waiting INSERT must not evaluate its sequence-backed default twice"
    );
}

#[test]
fn should_wait_for_a_unique_conflict_owner_before_returning_insert_when_owner_rolls_back() {
    // Arrange
    let fixture = sql_fixture(
        "tx-unique-conflict-wait-rollback",
        &[
            "CREATE TABLE tx_unique_wait_rollback (id SERIAL PRIMARY KEY, label TEXT UNIQUE)",
            "BEGIN",
            "INSERT INTO tx_unique_wait_rollback (label) VALUES ('same') RETURNING id",
        ],
    );
    let owner_session = fixture.session.clone();
    let waiter_session = fixture.cassie.create_session("waiter", None);
    let cassie = fixture.cassie.clone();
    fixture
        .cassie
        .execute_sql(&waiter_session, "BEGIN", vec![])
        .expect("begin the conflicting transaction");

    let (result_sender, result_receiver) = mpsc::sync_channel(1);
    let started = Arc::new(Barrier::new(2));
    let thread_started = Arc::clone(&started);
    let thread_session = waiter_session.clone();
    let waiter = std::thread::spawn(move || {
        thread_started.wait();
        let result = cassie.execute_sql(
            &thread_session,
            "INSERT INTO tx_unique_wait_rollback (label) VALUES ('same') RETURNING id",
            vec![],
        );
        result_sender
            .send(result)
            .expect("main test thread receives the insert result");
    });
    started.wait();
    let request_entered_engine = wait_for_running_query(&fixture.cassie);
    let before_owner_rollback = result_receiver.recv_timeout(Duration::from_millis(500));
    let returned_before_owner_rollback = before_owner_rollback.is_ok();

    // Act
    fixture
        .cassie
        .execute_sql(&owner_session, "ROLLBACK", vec![])
        .expect("release the unique-key owner transaction");
    let waiter_result = match before_owner_rollback {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => result_receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("conflicting insert completes after owner resolution"),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            panic!("conflicting insert worker exited without returning a result")
        }
    };
    waiter
        .join()
        .expect("conflicting insert worker should finish");
    let waiter_state = waiter_session.transaction_status();
    fixture
        .cassie
        .execute_sql(&waiter_session, "COMMIT", vec![])
        .expect("commit the insert after the owner rolled back");

    // Assert
    assert!(
        request_entered_engine,
        "the conflicting INSERT must enter the engine before owner rollback"
    );
    assert!(
        !returned_before_owner_rollback,
        "PostgreSQL waits for the unresolved unique-key owner before returning INSERT"
    );
    assert_eq!(
        waiter_result
            .expect("the rolled-back owner frees the key")
            .rows(),
        vec![vec![Value::Int64(2)]],
        "waiting must not evaluate the sequence-backed default twice"
    );
    assert_eq!(waiter_state, "in_transaction");
    assert_eq!(
        fixture.rows("SELECT id FROM tx_unique_wait_rollback ORDER BY id"),
        vec![vec![Value::Int64(2)]]
    );
}

#[test]
fn should_wait_for_each_unique_owner_in_a_multi_row_insert() {
    // Arrange
    let fixture = sql_fixture(
        "tx-unique-conflict-multiple-owners",
        &["CREATE TABLE tx_unique_wait_multiple (id INT PRIMARY KEY, label TEXT UNIQUE)"],
    );
    let first_owner = fixture.session.clone();
    begin_and_stage_unique_row(
        &fixture.cassie,
        &first_owner,
        "INSERT INTO tx_unique_wait_multiple VALUES (1, 'first')",
    );
    let second_owner = fixture.cassie.create_session("second-owner", None);
    begin_and_stage_unique_row(
        &fixture.cassie,
        &second_owner,
        "INSERT INTO tx_unique_wait_multiple VALUES (2, 'second')",
    );
    let waiter_session = fixture.cassie.create_session("waiter", None);
    fixture
        .cassie
        .execute_sql(&waiter_session, "BEGIN", vec![])
        .expect("begin the multi-row waiting transaction");
    let (waiter, result_receiver) =
        start_waiting_multi_row_insert(fixture.cassie.clone(), waiter_session.clone());
    let request_entered_engine = wait_for_running_query(&fixture.cassie);
    let before_first_owner_rollback = result_receiver.recv_timeout(Duration::from_millis(500));
    let returned_before_first_owner_rollback = before_first_owner_rollback.is_ok();

    // Act
    fixture
        .cassie
        .execute_sql(&first_owner, "ROLLBACK", vec![])
        .expect("release the first unique-key owner");
    let after_first_owner_rollback = if let Ok(result) = before_first_owner_rollback {
        Some(result)
    } else {
        match result_receiver.recv_timeout(Duration::from_millis(500)) {
            Ok(result) => Some(result),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                panic!("multi-row INSERT worker exited without returning a result")
            }
        }
    };
    let returned_while_second_owner_holds_key = after_first_owner_rollback.is_some();
    fixture
        .cassie
        .execute_sql(&second_owner, "ROLLBACK", vec![])
        .expect("release the second unique-key owner");
    let waiter_result = after_first_owner_rollback.unwrap_or_else(|| {
        result_receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("multi-row INSERT completes after both owners roll back")
    });
    waiter
        .join()
        .expect("multi-row INSERT worker should finish");
    let transaction_resolution = if waiter_result.is_ok() {
        "COMMIT"
    } else {
        "ROLLBACK"
    };
    fixture
        .cassie
        .execute_sql(&waiter_session, transaction_resolution, vec![])
        .expect("resolve the multi-row waiting transaction");

    // Assert
    assert!(
        request_entered_engine,
        "multi-row INSERT must enter the engine"
    );
    assert!(
        !returned_before_first_owner_rollback,
        "multi-row INSERT must wait for the first unique-key owner"
    );
    assert!(
        !returned_while_second_owner_holds_key,
        "resolving one key must leave the INSERT waiting for its other owner"
    );
    assert_eq!(
        waiter_result
            .expect("both owner rollbacks release their unique values")
            .rows(),
        vec![vec![Value::Int64(3)], vec![Value::Int64(4)]]
    );
    assert_eq!(
        fixture.rows("SELECT id FROM tx_unique_wait_multiple ORDER BY id"),
        vec![vec![Value::Int64(3)], vec![Value::Int64(4)]]
    );
}

#[test]
fn should_report_a_committed_conflict_before_waiting_on_other_insert_keys() {
    // Arrange
    let fixture = sql_fixture(
        "tx-unique-conflict-multi-owner-commit",
        &["CREATE TABLE tx_unique_wait_multiple (id INT PRIMARY KEY, label TEXT UNIQUE)"],
    );
    let first_owner = fixture.session.clone();
    begin_and_stage_unique_row(
        &fixture.cassie,
        &first_owner,
        "INSERT INTO tx_unique_wait_multiple VALUES (1, 'first')",
    );
    let second_owner = fixture.cassie.create_session("second-owner", None);
    begin_and_stage_unique_row(
        &fixture.cassie,
        &second_owner,
        "INSERT INTO tx_unique_wait_multiple VALUES (2, 'second')",
    );
    let waiter_session = fixture.cassie.create_session("waiter", None);
    fixture
        .cassie
        .execute_sql(&waiter_session, "BEGIN", vec![])
        .expect("begin the multi-row waiting transaction");
    let (waiter, result_receiver) =
        start_waiting_multi_row_insert(fixture.cassie.clone(), waiter_session.clone());
    let request_entered_engine = wait_for_running_query(&fixture.cassie);
    let before_first_owner_commit = result_receiver.recv_timeout(Duration::from_millis(500));
    let returned_before_first_owner_commit = before_first_owner_commit.is_ok();

    // Act
    fixture
        .cassie
        .execute_sql(&first_owner, "COMMIT", vec![])
        .expect("commit the owner of the first INSERT key");
    let after_first_owner_commit = result_receiver.recv_timeout(Duration::from_millis(500));
    let returned_before_second_owner_resolution = after_first_owner_commit.is_ok();
    fixture
        .cassie
        .execute_sql(&second_owner, "ROLLBACK", vec![])
        .expect("release the other unique-key owner");
    let waiter_result = match after_first_owner_commit {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => result_receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("multi-row INSERT completes after remaining owner resolution"),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            panic!("multi-row INSERT worker exited without returning a result")
        }
    };
    waiter
        .join()
        .expect("multi-row INSERT worker should finish");
    fixture
        .cassie
        .execute_sql(&waiter_session, "ROLLBACK", vec![])
        .expect("roll back the failed multi-row INSERT transaction");

    // Assert
    assert!(
        request_entered_engine,
        "multi-row INSERT must enter the engine"
    );
    assert!(
        !returned_before_first_owner_commit,
        "the multi-row INSERT must wait for the first owner"
    );
    assert!(
        returned_before_second_owner_resolution,
        "a committed duplicate must fail the INSERT without waiting for another key owner"
    );
    assert!(
        matches!(waiter_result, Err(CassieError::UniqueViolation { .. })),
        "the committed owner must produce a unique violation"
    );
    assert_eq!(
        fixture.rows("SELECT id FROM tx_unique_wait_multiple ORDER BY id"),
        vec![vec![Value::Int64(1)]]
    );
}

#[test]
fn should_release_unique_key_waiters_when_owner_rolls_back_to_savepoint() {
    // Arrange
    let fixture = sql_fixture(
        "tx-unique-conflict-savepoint",
        &[
            "CREATE TABLE tx_unique_wait_savepoint (id INT PRIMARY KEY, label TEXT UNIQUE)",
            "BEGIN",
            "SAVEPOINT before_insert",
            "INSERT INTO tx_unique_wait_savepoint (id, label) VALUES (1, 'same') RETURNING id",
        ],
    );
    let owner_session = fixture.session.clone();
    let waiter_session = fixture.cassie.create_session("waiter", None);
    let cassie = fixture.cassie.clone();
    fixture
        .cassie
        .execute_sql(&waiter_session, "BEGIN", vec![])
        .expect("begin the conflicting transaction");

    let (result_sender, result_receiver) = mpsc::sync_channel(1);
    let started = Arc::new(Barrier::new(2));
    let thread_started = Arc::clone(&started);
    let thread_session = waiter_session.clone();
    let waiter = std::thread::spawn(move || {
        thread_started.wait();
        let result = cassie.execute_sql(
            &thread_session,
            "INSERT INTO tx_unique_wait_savepoint (id, label) VALUES (2, 'same') RETURNING id",
            vec![],
        );
        result_sender
            .send(result)
            .expect("main test thread receives the insert result");
    });
    started.wait();
    let request_entered_engine = wait_for_running_query(&fixture.cassie);
    let before_savepoint_rollback = result_receiver.recv_timeout(Duration::from_millis(500));
    let returned_before_savepoint_rollback = before_savepoint_rollback.is_ok();

    // Act
    fixture
        .cassie
        .execute_sql(
            &owner_session,
            "ROLLBACK TO SAVEPOINT before_insert",
            vec![],
        )
        .expect("release the staged key while keeping the owner transaction open");
    let waiter_result = match before_savepoint_rollback {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => result_receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("conflicting insert completes after savepoint rollback"),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            panic!("conflicting insert worker exited without returning a result")
        }
    };
    waiter
        .join()
        .expect("conflicting insert worker should finish");
    fixture
        .cassie
        .execute_sql(&owner_session, "COMMIT", vec![])
        .expect("commit the transaction after rolling back to savepoint");
    fixture
        .cassie
        .execute_sql(&waiter_session, "COMMIT", vec![])
        .expect("commit the insert after the owner rolled back to savepoint");

    // Assert
    assert!(
        request_entered_engine,
        "the conflicting INSERT must enter the engine before savepoint rollback"
    );
    assert!(
        !returned_before_savepoint_rollback,
        "PostgreSQL waits for the unresolved unique-key owner until savepoint rollback"
    );
    assert!(
        waiter_result.is_ok(),
        "savepoint rollback frees the staged key"
    );
    assert_eq!(
        fixture.rows("SELECT id FROM tx_unique_wait_savepoint ORDER BY id"),
        vec![vec![Value::Int64(2)]]
    );
}

#[test]
fn should_release_unique_key_waiters_when_owner_session_drops() {
    // Arrange
    let fixture = sql_fixture(
        "tx-unique-conflict-session-drop",
        &["CREATE TABLE tx_unique_wait_drop (id INT PRIMARY KEY, label TEXT UNIQUE)"],
    );
    let owner_session = fixture.cassie.create_session("owner", None);
    for sql in [
        "BEGIN",
        "INSERT INTO tx_unique_wait_drop (id, label) VALUES (1, 'same')",
    ] {
        fixture
            .cassie
            .execute_sql(&owner_session, sql, vec![])
            .expect("stage the unique key for the owner");
    }
    let waiter_session = fixture.cassie.create_session("waiter", None);
    fixture
        .cassie
        .execute_sql(&waiter_session, "BEGIN", vec![])
        .expect("begin the waiting transaction");
    let cassie = fixture.cassie.clone();
    let thread_session = waiter_session.clone();
    let (result_sender, result_receiver) = mpsc::sync_channel(1);
    let started = Arc::new(Barrier::new(2));
    let thread_started = Arc::clone(&started);
    let waiter = std::thread::spawn(move || {
        thread_started.wait();
        let result = cassie.execute_sql(
            &thread_session,
            "INSERT INTO tx_unique_wait_drop (id, label) VALUES (2, 'same') RETURNING id",
            vec![],
        );
        result_sender
            .send(result)
            .expect("main test thread receives the insert result");
    });
    started.wait();
    let request_entered_engine = wait_for_running_query(&fixture.cassie);
    let before_owner_drop = result_receiver.recv_timeout(Duration::from_millis(500));
    let returned_before_owner_drop = before_owner_drop.is_ok();

    // Act
    drop(owner_session);
    let waiter_result = match before_owner_drop {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => result_receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("waiting insert completes after owner session drop"),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            panic!("conflicting insert worker exited without returning a result")
        }
    };
    waiter
        .join()
        .expect("conflicting insert worker should finish");
    fixture
        .cassie
        .execute_sql(&waiter_session, "COMMIT", vec![])
        .expect("commit the insert after the owner session drops");

    // Assert
    assert!(request_entered_engine, "the INSERT must enter the engine");
    assert!(
        !returned_before_owner_drop,
        "PostgreSQL waits until the transaction owner exits"
    );
    assert!(waiter_result.is_ok(), "dropping the owner releases its key");
    assert_eq!(
        fixture.rows("SELECT id FROM tx_unique_wait_drop ORDER BY id"),
        vec![vec![Value::Int64(2)]]
    );
}

