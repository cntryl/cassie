use super::*;

#[test]
fn should_respect_membership_inherit_option_while_allowing_set_role() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("role_membership_inherit_option").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE ROLE compat_inherit_option_group NOLOGIN",
                "CREATE ROLE compat_inherit_option_member NOLOGIN",
                "CREATE TABLE compat_inherit_option_rows (id INT)",
                "INSERT INTO compat_inherit_option_rows VALUES (1)",
                "GRANT SELECT ON TABLE compat_inherit_option_rows \
                   TO compat_inherit_option_group",
                "GRANT compat_inherit_option_group TO compat_inherit_option_member \
                   WITH INHERIT FALSE",
            ],
        )
        .await
        .expect("create a role membership that disables privilege inheritance");
        let membership = client
            .query_one(
                "SELECT membership.admin_option, membership.inherit_option, \
                        membership.set_option, \
                        pg_catalog.pg_has_role(member.rolname, granted_role.rolname, 'MEMBER'), \
                        pg_catalog.pg_has_role(member.rolname, granted_role.rolname, 'USAGE'), \
                        pg_catalog.pg_has_role(member.rolname, granted_role.rolname, 'SET'), \
                        pg_catalog.has_table_privilege( \
                          member.rolname, 'public.compat_inherit_option_rows', 'SELECT') \
                 FROM pg_catalog.pg_auth_members AS membership \
                 JOIN pg_catalog.pg_roles AS member \
                   ON member.oid = membership.member \
                 JOIN pg_catalog.pg_roles AS granted_role \
                   ON granted_role.oid = membership.roleid \
                 WHERE member.rolname = 'compat_inherit_option_member' \
                   AND granted_role.rolname = 'compat_inherit_option_group'",
                &[],
            )
            .await
            .expect("inspect the membership options and inherited table privilege");

        // Act
        client
            .batch_execute("SET ROLE compat_inherit_option_member")
            .await
            .expect("assume the role without inherited table privileges");
        let member_privilege: bool = client
            .query_one(
                "SELECT pg_catalog.has_table_privilege( \
                   current_user, 'public.compat_inherit_option_rows', 'SELECT')",
                &[],
            )
            .await
            .expect("check table privilege while the member role is active")
            .get(0);
        let denied_read = client
            .query("SELECT id FROM compat_inherit_option_rows ORDER BY id", &[])
            .await
            .expect_err("the member must not inherit SELECT through this membership");
        client
            .batch_execute("SET ROLE compat_inherit_option_group")
            .await
            .expect("the SET membership option must permit role assumption");
        let group_rows = client
            .query("SELECT id FROM compat_inherit_option_rows ORDER BY id", &[])
            .await
            .expect("read the granted row after assuming the member role");

        // Assert
        assert!(!membership.get::<_, bool>(0));
        assert!(!membership.get::<_, bool>(1));
        assert!(membership.get::<_, bool>(2));
        assert!(membership.get::<_, bool>(3));
        assert!(!membership.get::<_, bool>(4));
        assert!(membership.get::<_, bool>(5));
        assert!(!membership.get::<_, bool>(6));
        assert!(!member_privilege);
        assert_eq!(db_error(&denied_read).code().code(), "42501");
        assert_eq!(group_rows.len(), 1);
        assert_eq!(group_rows[0].get::<_, i32>(0), 1);

        drop(client);
        server.shutdown(connection).await;
    });
}
