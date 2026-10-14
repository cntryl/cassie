use super::*;

#[test]
fn should_run_security_definer_trigger_in_its_pinned_relation_context() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("security_definer_trigger_context").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE ROLE compat_trigger_context_reader NOLOGIN",
                "CREATE SCHEMA compat_trigger_context_trusted",
                "CREATE TABLE compat_trigger_context_trusted.source (id INT PRIMARY KEY)",
                "CREATE TABLE compat_trigger_context_trusted.trigger_audit \
                   (id INT, executor TEXT)",
                "CREATE FUNCTION compat_trigger_context_trusted.capture_insert() \
                   RETURNS TRIGGER LANGUAGE plpgsql SECURITY DEFINER \
                   SET search_path = compat_trigger_context_trusted, pg_temp \
                   AS $$ BEGIN \
                     INSERT INTO trigger_audit VALUES (NEW.id, current_user::TEXT); \
                     RETURN NEW; \
                   END $$",
                "CREATE TRIGGER compat_trigger_context_capture \
                   AFTER INSERT ON compat_trigger_context_trusted.source \
                   FOR EACH ROW EXECUTE FUNCTION \
                     compat_trigger_context_trusted.capture_insert()",
                "GRANT USAGE ON SCHEMA compat_trigger_context_trusted \
                   TO compat_trigger_context_reader",
                "GRANT INSERT ON compat_trigger_context_trusted.source \
                   TO compat_trigger_context_reader",
                "SET search_path TO public, pg_temp",
                "SET ROLE compat_trigger_context_reader",
                "CREATE TEMP TABLE trigger_audit (id INT, executor TEXT)",
                "INSERT INTO trigger_audit VALUES (77, 'caller')",
            ],
        )
        .await
        .expect("create a protected trigger target and a caller-controlled temp shadow");

        // Act
        let unauthorized_write = client
            .execute(
                "INSERT INTO compat_trigger_context_trusted.trigger_audit \
                 VALUES (99, 'direct')",
                &[],
            )
            .await
            .expect_err("the caller must not write directly to the protected audit relation");
        client
            .execute(
                "INSERT INTO compat_trigger_context_trusted.source VALUES (1)",
                &[],
            )
            .await
            .expect("the caller has permission to insert into the trigger target");
        let caller_shadow = client
            .query_one("SELECT id, executor FROM trigger_audit", &[])
            .await
            .expect("the caller continues to resolve its own temporary shadow");
        let caller_path: String = client
            .query_one("SHOW search_path", &[])
            .await
            .expect("read the caller search path after trigger execution")
            .try_get(0)
            .expect("caller search path");
        client
            .batch_execute("RESET ROLE")
            .await
            .expect("restore the administrative role");
        let protected_audit = client
            .query(
                "SELECT id, executor \
                 FROM compat_trigger_context_trusted.trigger_audit ORDER BY id",
                &[],
            )
            .await
            .expect("read the persistent audit relation after trigger execution");

        // Assert
        assert_eq!(db_error(&unauthorized_write).code().code(), "42501");
        assert_eq!(caller_shadow.get::<_, i32>(0), 77);
        assert_eq!(caller_shadow.get::<_, String>(1), "caller");
        assert_eq!(caller_path, "public, pg_temp");
        assert_eq!(protected_audit.len(), 1);
        assert_eq!(protected_audit[0].get::<_, i32>(0), 1);
        assert_eq!(protected_audit[0].get::<_, String>(1), "root");

        drop(client);
        server.shutdown(connection).await;
    });
}
