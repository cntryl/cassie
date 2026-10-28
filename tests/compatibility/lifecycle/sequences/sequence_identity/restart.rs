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
