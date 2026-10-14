use super::*;

#[test]
fn should_fire_row_triggers_for_copy_from_stdin_rows() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_row_trigger_interaction").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_copy_trigger_source \
                   (id INT PRIMARY KEY, label TEXT NOT NULL)",
                "CREATE TABLE compat_copy_trigger_audit (id INT, label TEXT)",
                "CREATE FUNCTION compat_copy_trigger_capture() RETURNS TRIGGER \
                   LANGUAGE plpgsql AS $$ BEGIN \
                     INSERT INTO compat_copy_trigger_audit VALUES (NEW.id, NEW.label); \
                     RETURN NEW; \
                   END $$",
                "CREATE TRIGGER compat_copy_trigger_capture_row \
                   AFTER INSERT ON compat_copy_trigger_source FOR EACH ROW \
                   EXECUTE FUNCTION compat_copy_trigger_capture()",
            ],
        )
        .await
        .expect("create source, audit table, and row trigger before COPY");

        // Act
        let (response_tag, _response_payload, error_code, command) = super::copy_from_stdin(
            &server,
            "COPY compat_copy_trigger_source (id, label) FROM STDIN WITH (FORMAT csv)",
            b"1,alpha\n2,beta\n",
        )
        .await;
        let source_rows = client
            .query(
                "SELECT id, label FROM compat_copy_trigger_source ORDER BY id",
                &[],
            )
            .await
            .expect("read rows imported through COPY");
        let audit_rows = client
            .query(
                "SELECT id, label FROM compat_copy_trigger_audit ORDER BY id",
                &[],
            )
            .await
            .expect("read trigger effects for the imported rows");

        // Assert
        assert_eq!(response_tag, b'G');
        assert_eq!(error_code, None);
        assert_eq!(command.as_deref(), Some("COPY 2"));
        assert_eq!(source_rows.len(), 2);
        assert_eq!(source_rows[0].get::<_, i32>(0), 1);
        assert_eq!(source_rows[0].get::<_, String>(1), "alpha");
        assert_eq!(source_rows[1].get::<_, i32>(0), 2);
        assert_eq!(source_rows[1].get::<_, String>(1), "beta");
        assert_eq!(audit_rows.len(), 2);
        assert_eq!(audit_rows[0].get::<_, i32>(0), 1);
        assert_eq!(audit_rows[0].get::<_, String>(1), "alpha");
        assert_eq!(audit_rows[1].get::<_, i32>(0), 2);
        assert_eq!(audit_rows[1].get::<_, String>(1), "beta");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_roll_back_copy_rows_when_a_row_trigger_raises() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("copy_row_trigger_atomicity").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_copy_trigger_failure_source (id INT PRIMARY KEY)",
                "CREATE TABLE compat_copy_trigger_failure_audit (id INT)",
                "CREATE FUNCTION compat_copy_trigger_reject_second() RETURNS TRIGGER \
                   LANGUAGE plpgsql AS $$ BEGIN \
                     INSERT INTO compat_copy_trigger_failure_audit VALUES (NEW.id); \
                     IF NEW.id = 2 THEN \
                       RAISE EXCEPTION 'reject the second imported row'; \
                     END IF; \
                     RETURN NEW; \
                   END $$",
                "CREATE TRIGGER compat_copy_trigger_reject_second_row \
                   AFTER INSERT ON compat_copy_trigger_failure_source FOR EACH ROW \
                   EXECUTE FUNCTION compat_copy_trigger_reject_second()",
            ],
        )
        .await
        .expect("create the COPY target, audit table, and rejecting row trigger");

        // Act
        let (response_tag, _response_payload, error_code, command) = super::copy_from_stdin(
            &server,
            "COPY compat_copy_trigger_failure_source (id) FROM STDIN WITH (FORMAT csv)",
            b"1\n2\n",
        )
        .await;
        let source_count: i64 = client
            .query_one(
                "SELECT count(*) FROM compat_copy_trigger_failure_source",
                &[],
            )
            .await
            .expect("read the COPY target after the trigger error")
            .get(0);
        let audit_count: i64 = client
            .query_one(
                "SELECT count(*) FROM compat_copy_trigger_failure_audit",
                &[],
            )
            .await
            .expect("read trigger effects after the COPY error")
            .get(0);

        // Assert
        assert_eq!(response_tag, b'G');
        assert_eq!(error_code.as_deref(), Some("P0001"));
        assert_eq!(command, None);
        assert_eq!(source_count, 0);
        assert_eq!(audit_count, 0);

        drop(client);
        server.shutdown(connection).await;
    });
}
