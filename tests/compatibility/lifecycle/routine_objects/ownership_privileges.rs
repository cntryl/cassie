use super::*;

mod column_privilege_scope;
mod grant_option_cascade;
mod local_role_scope;
mod role_membership_inherit_option;
mod row_security_force_owner;
mod session_authorization;

#[test]
fn should_report_ungranted_object_privileges_as_false() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("ungranted_object_privileges").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE ROLE compat_ungranted_reader NOLOGIN",
                "CREATE SCHEMA compat_ungranted_schema",
                "CREATE TABLE compat_ungranted_schema.rows (id INT)",
            ],
        )
        .await
        .expect("create role and objects without granting privileges");

        // Act
        let privileges = client
            .query_one(
                "SELECT has_database_privilege('compat_ungranted_reader', 'postgres', 'CREATE'), \
                        has_schema_privilege('compat_ungranted_reader', 'compat_ungranted_schema', 'USAGE'), \
                        has_table_privilege('compat_ungranted_reader', 'compat_ungranted_schema.rows', 'SELECT')",
                &[],
            )
            .await
            .expect("check privileges for the ungranted role");

        // Assert
        assert_eq!(
            (
                privileges.get::<_, bool>(0),
                privileges.get::<_, bool>(1),
                privileges.get::<_, bool>(2),
            ),
            (false, false, false)
        );

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_report_unrelated_roles_are_not_members() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("unrelated_role_membership").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE ROLE compat_unrelated_member NOLOGIN",
                "CREATE ROLE compat_unrelated_target NOLOGIN",
            ],
        )
        .await
        .expect("create independent roles without a membership grant");

        // Act
        let membership = client
            .query_one(
                "SELECT pg_has_role('compat_unrelated_member', 'compat_unrelated_target', 'MEMBER')",
                &[],
            )
            .await
            .expect("check membership between unrelated roles");

        // Assert
        assert!(!membership.get::<_, bool>(0));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_report_transferred_relation_owner_in_catalogs() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("relation_owner_transfer").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE ROLE compat_relation_owner NOLOGIN",
                "CREATE TABLE compat_owner_transfer_rows (id INT)",
            ],
        )
        .await
        .expect("create a target role and relation");

        // Act
        client
            .batch_execute("ALTER TABLE compat_owner_transfer_rows OWNER TO compat_relation_owner")
            .await
            .expect("transfer the table to the target owner");
        let owner: String = client
            .query_one(
                "SELECT pg_get_userbyid(relowner) FROM pg_class \
                 WHERE relname = 'compat_owner_transfer_rows'",
                &[],
            )
            .await
            .expect("read the relation owner from pg_class")
            .try_get(0)
            .expect("relation owner name");

        // Assert
        assert_eq!(owner, "compat_relation_owner");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_transfer_function_owner_in_catalog() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("function_owner_catalog").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE ROLE compat_function_owner NOLOGIN",
                "CREATE FUNCTION compat_privilege_lookup() RETURNS INT \
                   IMMUTABLE AS \"11\"",
            ],
        )
        .await
        .expect("create a target owner and a scalar SQL routine");

        // Act
        client
            .batch_execute(
                "ALTER FUNCTION compat_privilege_lookup() \
                 OWNER TO compat_function_owner",
            )
            .await
            .expect("transfer the routine to the target owner");
        let owner: String = client
            .query_one(
                "SELECT pg_get_userbyid(proowner) \
                 FROM pg_proc \
                 WHERE oid = 'public.compat_privilege_lookup()'::regprocedure",
                &[],
            )
            .await
            .expect("read the function owner from pg_proc")
            .try_get(0)
            .expect("function owner name");

        // Assert
        assert_eq!(owner, "compat_function_owner");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_limit_function_execute_grant_to_its_grantee() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("function_execute_grant").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE ROLE compat_function_reader NOLOGIN",
                "CREATE ROLE compat_function_stranger NOLOGIN",
                "CREATE FUNCTION compat_privilege_lookup() RETURNS INT \
                   IMMUTABLE AS \"11\"",
            ],
        )
        .await
        .expect("create grantee roles and a scalar SQL routine");

        // Act
        client
            .batch_execute("REVOKE ALL ON FUNCTION compat_privilege_lookup() FROM PUBLIC")
            .await
            .expect("remove the default public EXECUTE grant");
        client
            .batch_execute(
                "GRANT EXECUTE ON FUNCTION compat_privilege_lookup() \
                 TO compat_function_reader",
            )
            .await
            .expect("grant EXECUTE only to the reader role");
        let privileges = client
            .query_one(
                "SELECT has_function_privilege( \
                          'compat_function_reader', \
                          'public.compat_privilege_lookup()', 'EXECUTE'), \
                        has_function_privilege( \
                          'compat_function_stranger', \
                          'public.compat_privilege_lookup()', 'EXECUTE')",
                &[],
            )
            .await
            .expect("inspect per-role function EXECUTE privileges");

        // Assert
        assert!(privileges.get::<_, bool>(0));
        assert!(!privileges.get::<_, bool>(1));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_toggle_force_row_security_catalog_flag() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("force_rls_catalog_flag").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_force_rls_rows (id INT PRIMARY KEY)",
                "ALTER TABLE compat_force_rls_rows FORCE ROW LEVEL SECURITY",
            ],
        )
        .await
        .expect("force row security on an existing table");

        // Act
        let forced = client
            .query_one(
                "SELECT relrowsecurity, relforcerowsecurity \
                 FROM pg_class WHERE relname = 'compat_force_rls_rows'",
                &[],
            )
            .await
            .expect("read row-security catalog flags after FORCE");
        client
            .batch_execute("ALTER TABLE compat_force_rls_rows NO FORCE ROW LEVEL SECURITY")
            .await
            .expect("turn off forced row security");
        let no_longer_forced = client
            .query_one(
                "SELECT relrowsecurity, relforcerowsecurity \
                 FROM pg_class WHERE relname = 'compat_force_rls_rows'",
                &[],
            )
            .await
            .expect("read row-security catalog flags after NO FORCE");

        // Assert
        assert!(!forced.get::<_, bool>(0));
        assert!(forced.get::<_, bool>(1));
        assert!(!no_longer_forced.get::<_, bool>(0));
        assert!(!no_longer_forced.get::<_, bool>(1));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_store_restrictive_update_policy_with_check_metadata() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("restrictive_policy_metadata").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE TABLE compat_policy_catalog_rows (id INT PRIMARY KEY)",
                "CREATE POLICY compat_policy_catalog_update \
                   ON compat_policy_catalog_rows AS RESTRICTIVE FOR UPDATE \
                   USING (id > 0) WITH CHECK (id > 0)",
            ],
        )
        .await
        .expect("create a restrictive UPDATE policy with a WITH CHECK expression");

        // Act
        let policy = client
            .query_one(
                "SELECT polpermissive, polcmd, polqual IS NOT NULL, \
                        polwithcheck IS NOT NULL \
                 FROM pg_policy WHERE polname = 'compat_policy_catalog_update'",
                &[],
            )
            .await
            .expect("read the policy command and stored expressions");

        // Assert
        assert!(!policy.get::<_, bool>(0));
        assert_eq!(policy.get::<_, String>(1), "w");
        assert!(policy.get::<_, bool>(2));
        assert!(policy.get::<_, bool>(3));

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_reject_updates_that_violate_a_policy_with_check_expression() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("rls_with_check_update").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE ROLE compat_with_check_reader NOLOGIN",
                "CREATE TABLE compat_with_check_rows \
                   (id INT PRIMARY KEY, owner_name TEXT NOT NULL)",
                "INSERT INTO compat_with_check_rows VALUES \
                   (1, 'compat_with_check_reader'), (2, 'another_owner')",
                "ALTER TABLE compat_with_check_rows ENABLE ROW LEVEL SECURITY",
                "CREATE POLICY compat_with_check_select_policy \
                   ON compat_with_check_rows FOR SELECT \
                   TO compat_with_check_reader \
                   USING (owner_name = current_user)",
                "CREATE POLICY compat_with_check_update_policy \
                   ON compat_with_check_rows FOR UPDATE \
                   TO compat_with_check_reader \
                   USING (owner_name = current_user) \
                   WITH CHECK (owner_name = current_user)",
                "GRANT SELECT, UPDATE ON compat_with_check_rows \
                   TO compat_with_check_reader",
                "SET ROLE compat_with_check_reader",
            ],
        )
        .await
        .expect("create row visibility and update-check policies for the reader");

        // Act
        let visible = client
            .query("SELECT id FROM compat_with_check_rows ORDER BY id", &[])
            .await
            .expect("read only the row allowed by the SELECT policy");
        let update_error = client
            .execute(
                "UPDATE compat_with_check_rows SET owner_name = 'another_owner' \
                 WHERE id = 1",
                &[],
            )
            .await
            .expect_err("reject an update whose new row violates WITH CHECK");
        let unchanged_owner: String = client
            .query_one(
                "SELECT owner_name FROM compat_with_check_rows WHERE id = 1",
                &[],
            )
            .await
            .expect("read the original row after the rejected update")
            .try_get(0)
            .expect("owner name after rejected update");

        // Assert
        assert_eq!(
            visible
                .iter()
                .map(|row| row.get::<_, i32>(0))
                .collect::<Vec<_>>(),
            vec![1]
        );
        assert_eq!(db_error(&update_error).code().code(), "42501");
        assert_eq!(unchanged_owner, "compat_with_check_reader");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_capture_function_setting_from_current_then_restore_caller_value() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("routine_setting_from_current").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute(
                "CREATE FUNCTION compat_captured_application_name() RETURNS TEXT \
                 STABLE AS \"current_setting('application_name')\"",
            )
            .await
            .expect("create a scalar routine on the supported UDF path");
        client
            .batch_execute("SET application_name = 'captured-at-alter'")
            .await
            .expect("set the value that ALTER FUNCTION should capture");

        // Act
        client
            .batch_execute(
                "ALTER FUNCTION compat_captured_application_name() \
                 SET application_name FROM CURRENT",
            )
            .await
            .expect("capture the current setting on the routine");
        client
            .batch_execute("SET application_name = 'caller-before-call'")
            .await
            .expect("restore a distinct caller setting before invoking the routine");
        let captured: String = client
            .query_one("SELECT compat_captured_application_name()", &[])
            .await
            .expect("call the function with its captured application_name")
            .try_get(0)
            .expect("captured application name");
        let caller_value: String = client
            .query_one("SHOW application_name", &[])
            .await
            .expect("read application_name after the function returns")
            .try_get(0)
            .expect("caller application name");

        // Assert
        assert_eq!(captured, "captured-at-alter");
        assert_eq!(caller_value, "caller-before-call");

        drop(client);
        server.shutdown(connection).await;
    });
}

#[test]
fn should_restore_caller_identity_after_nested_altered_security_definer_call() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("nested_security_definer_context").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE ROLE compat_nested_security_caller NOLOGIN",
                "CREATE FUNCTION compat_nested_security_inner() RETURNS TEXT \
                   STABLE AS \"current_user\"",
                "CREATE FUNCTION compat_nested_security_outer() RETURNS TEXT \
                   STABLE AS \"compat_nested_security_inner()\"",
            ],
        )
        .await
        .expect("create nested SQL routines on the supported UDF path");

        // Act
        client
            .batch_execute("ALTER FUNCTION compat_nested_security_outer() SECURITY DEFINER")
            .await
            .expect("change the outer routine to SECURITY DEFINER");
        client
            .batch_execute("REVOKE ALL ON FUNCTION compat_nested_security_outer() FROM PUBLIC")
            .await
            .expect("remove the public execute grant");
        client
            .batch_execute(
                "GRANT EXECUTE ON FUNCTION compat_nested_security_outer() \
                 TO compat_nested_security_caller",
            )
            .await
            .expect("grant the nested routine to its caller role");
        client
            .batch_execute("SET ROLE compat_nested_security_caller")
            .await
            .expect("assume the routine's caller role");
        let context = client
            .query_one(
                "SELECT current_user, compat_nested_security_outer(), current_user",
                &[],
            )
            .await
            .expect("call the altered SECURITY DEFINER routine as its grantee");

        // Assert
        assert_eq!(context.get::<_, String>(0), "compat_nested_security_caller");
        assert_eq!(context.get::<_, String>(1), "root");
        assert_eq!(context.get::<_, String>(2), "compat_nested_security_caller");

        drop(client);
        server.shutdown(connection).await;
    });
}
