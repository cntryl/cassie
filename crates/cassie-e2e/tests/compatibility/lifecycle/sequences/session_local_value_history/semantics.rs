use super::*;

fn assert_sequence_session_history(
    first_a: i64,
    first_b: i64,
    second_currval_error: &tokio_postgres::Error,
    second_lastval_error: &tokio_postgres::Error,
    second_a: i64,
    first_history: &tokio_postgres::Row,
    second_history: &tokio_postgres::Row,
) {
    assert_eq!(first_a, 10);
    assert_eq!(first_b, 100);
    assert_eq!(db_error(second_currval_error).code().code(), "55000");
    assert_eq!(db_error(second_lastval_error).code().code(), "55000");
    assert_eq!(second_a, 15);
    assert_eq!(
        (
            first_history.get::<_, i64>(0),
            first_history.get::<_, i64>(1),
            first_history.get::<_, i64>(2),
        ),
        (10, 100, 100),
        "another session's allocation does not replace this session's currval or lastval"
    );
    assert_eq!(
        (
            second_history.get::<_, i64>(0),
            second_history.get::<_, i64>(1),
        ),
        (15, 15)
    );
    assert!(first_history
        .columns()
        .iter()
        .all(|column| column.type_().oid() == 20));
    assert!(second_history
        .columns()
        .iter()
        .all(|column| column.type_().oid() == 20));
}

#[test]
fn should_keep_sequence_value_history_session_local() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("sequence_session_value_history").await;
        let (first_session, first_connection) = server.connect().await;
        let (second_session, second_connection) = server.connect().await;
        execute_statements(
            &first_session,
            &[
                "CREATE SEQUENCE compat_wave7_session_sequence_a \
                 START WITH 10 INCREMENT BY 5",
                "CREATE SEQUENCE compat_wave7_session_sequence_b \
                 START WITH 100 INCREMENT BY 10",
            ],
        )
        .await
        .expect("create two shared sequences with distinguishable values");
        let first_a: i64 = first_session
            .query_one("SELECT nextval('compat_wave7_session_sequence_a')", &[])
            .await
            .expect("allocate from the first sequence in the first session")
            .try_get(0)
            .expect("first session's first sequence value");
        let first_b: i64 = first_session
            .query_one("SELECT nextval('compat_wave7_session_sequence_b')", &[])
            .await
            .expect("allocate from the second sequence in the first session")
            .try_get(0)
            .expect("first session's second sequence value");
        let second_currval_before_nextval = second_session
            .query_one("SELECT currval('compat_wave7_session_sequence_a')", &[])
            .await
            .expect_err("currval is unset until this session calls nextval");
        let second_lastval_before_nextval = second_session
            .query_one("SELECT lastval()", &[])
            .await
            .expect_err("lastval is unset until this session calls nextval");

        // Act
        let second_a: i64 = second_session
            .query_one("SELECT nextval('compat_wave7_session_sequence_a')", &[])
            .await
            .expect("allocate from the shared sequence in the second session")
            .try_get(0)
            .expect("second session's sequence value");
        let first_history = first_session
            .query_one(
                "SELECT currval('compat_wave7_session_sequence_a'), \
                        currval('compat_wave7_session_sequence_b'), lastval()",
                &[],
            )
            .await
            .expect("read the first session's per-sequence and last-value history");
        let second_history = second_session
            .query_one(
                "SELECT currval('compat_wave7_session_sequence_a'), lastval()",
                &[],
            )
            .await
            .expect("read the second session's sequence history");
        first_session
            .batch_execute(
                "DROP SEQUENCE compat_wave7_session_sequence_a, \
                 compat_wave7_session_sequence_b",
            )
            .await
            .expect("drop both sequence fixtures");

        // Assert
        assert_sequence_session_history(
            first_a,
            first_b,
            &second_currval_before_nextval,
            &second_lastval_before_nextval,
            second_a,
            &first_history,
            &second_history,
        );

        drop((first_session, second_session));
        server.shutdown(first_connection).await;
        second_connection.abort();
        let _ = second_connection.await;
    });
}
