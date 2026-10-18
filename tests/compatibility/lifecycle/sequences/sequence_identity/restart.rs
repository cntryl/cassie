fn assert_sequence_restart_and_setval_lifecycle(
    first: &tokio_postgres::Row,
    restarted: &tokio_postgres::Row,
    last_after_restart_rollback: &tokio_postgres::Row,
    next_after_restart_rollback: &tokio_postgres::Row,
    set_value: &tokio_postgres::Row,
    after_setval_rollback: &tokio_postgres::Row,
) {
    assert_eq!(first.get::<_, i64>(0), 10);
    assert_eq!(first.columns()[0].type_().oid(), 20);
    assert_eq!(restarted.get::<_, i64>(0), 50);
    assert_eq!(restarted.columns()[0].type_().oid(), 20);
    assert_eq!(
        (
            last_after_restart_rollback.get::<_, i64>(0),
            last_after_restart_rollback.get::<_, i64>(1),
            next_after_restart_rollback.get::<_, i64>(0),
        ),
        (50, 50, 11),
        "ALTER SEQUENCE RESTART rolls back while session value history remains"
    );
    assert_eq!(set_value.get::<_, i64>(0), 80);
    assert_eq!(set_value.columns()[0].type_().oid(), 20);
    assert_eq!(
        (
            after_setval_rollback.get::<_, i64>(0),
            after_setval_rollback.get::<_, i64>(1),
            after_setval_rollback.get::<_, i64>(2),
        ),
        (80, 80, 81),
        "setval remains effective after transaction rollback"
    );
    assert!(after_setval_rollback
        .columns()
        .iter()
        .all(|column| column.type_().oid() == 20));
}

#[test]
fn should_rollback_sequence_restart_but_keep_setval_across_transaction_rollback() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("sequence_restart_setval_rollback").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE SEQUENCE compat_wave7_transactional_sequence START WITH 10")
            .await
            .expect("create a sequence with a known starting value");
        let first = client
            .query_one("SELECT nextval('compat_wave7_transactional_sequence')", &[])
            .await
            .expect("allocate the initial sequence value");

        // Act
        client
            .batch_execute("BEGIN")
            .await
            .expect("begin a transaction for ALTER SEQUENCE RESTART");
        client
            .batch_execute("ALTER SEQUENCE compat_wave7_transactional_sequence RESTART WITH 50")
            .await
            .expect("restart the sequence inside the transaction");
        let restarted = client
            .query_one("SELECT nextval('compat_wave7_transactional_sequence')", &[])
            .await
            .expect("read the transactional restart value");
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back the sequence restart");
        let last_after_restart_rollback = client
            .query_one(
                "SELECT currval('compat_wave7_transactional_sequence'), lastval()",
                &[],
            )
            .await
            .expect("retain the session's returned value after rollback");
        let next_after_restart_rollback = client
            .query_one("SELECT nextval('compat_wave7_transactional_sequence')", &[])
            .await
            .expect("continue from the pre-transaction sequence state");

        client
            .batch_execute("BEGIN")
            .await
            .expect("begin a transaction for setval");
        let set_value = client
            .query_one(
                "SELECT setval('compat_wave7_transactional_sequence', 80, true)",
                &[],
            )
            .await
            .expect("set the sequence value inside the transaction");
        client
            .batch_execute("ROLLBACK")
            .await
            .expect("roll back the transaction containing setval");
        let after_setval_rollback = client
            .query_one(
                "SELECT currval('compat_wave7_transactional_sequence'), \
                        lastval(), nextval('compat_wave7_transactional_sequence')",
                &[],
            )
            .await
            .expect("observe the nontransactional setval state");

        // Assert
        assert_sequence_restart_and_setval_lifecycle(
            &first,
            &restarted,
            &last_after_restart_rollback,
            &next_after_restart_rollback,
            &set_value,
            &after_setval_rollback,
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
#[test]
fn should_restore_sequence_state_after_server_restart() {
    // Arrange
    support::use_local_storage();
    let path = support::data_dir("sequence-allocation-restart");
    let _directory = DataDirectory(path.clone());
    let first_values = {
        let cassie = Cassie::new_with_data_dir(&path).expect("create local Cassie instance");
        cassie.startup().expect("start local Cassie instance");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE SEQUENCE compat_wave7_durable_sequence \
                 START WITH 41 INCREMENT BY 3 CACHE 1",
                vec![],
            )
            .expect("create a sequence with persistent options");
        let values = cassie
            .execute_sql(
                &session,
                "SELECT nextval('compat_wave7_durable_sequence'), \
                        nextval('compat_wave7_durable_sequence')",
                vec![],
            )
            .expect("allocate two sequence values before restart")
            .rows()
            .to_vec();
        drop(session);
        drop(cassie);
        values
    };

    // Act
    let reopened = Cassie::new_with_data_dir(&path).expect("reopen local Cassie instance");
    reopened.startup().expect("recover local Cassie instance");
    let session = reopened.create_session("tester", None);
    let recovered_value = reopened
        .execute_sql(
            &session,
            "SELECT nextval('compat_wave7_durable_sequence')",
            vec![],
        )
        .expect("allocate the next sequence value after restart");
    let recovered_options = reopened
        .execute_sql(
            &session,
            "SELECT seqstart, seqincrement, seqcache \
             FROM pg_catalog.pg_sequence \
             WHERE seqrelid = 'compat_wave7_durable_sequence'::regclass",
            vec![],
        )
        .expect("read sequence options after restart");

    // Assert
    assert_eq!(
        first_values,
        vec![vec![
            cassie::types::Value::Int64(41),
            cassie::types::Value::Int64(44),
        ]]
    );
    assert_eq!(
        recovered_value.rows(),
        &[vec![cassie::types::Value::Int64(47)]]
    );
    assert_eq!(
        recovered_options.rows(),
        &[vec![
            cassie::types::Value::Int64(41),
            cassie::types::Value::Int64(3),
            cassie::types::Value::Int64(1),
        ]]
    );
}
