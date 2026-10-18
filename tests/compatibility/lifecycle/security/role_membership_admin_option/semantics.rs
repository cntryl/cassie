use super::*;

#[test]
fn should_revoke_role_admin_option_without_revoking_membership() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("role_membership_admin_option").await;
        let (client, connection) = server.connect().await;
        execute_statements(
            &client,
            &[
                "CREATE ROLE compat_admin_option_role NOLOGIN",
                "CREATE ROLE compat_admin_option_member NOLOGIN",
            ],
        )
        .await
        .expect("create roles for the membership grant");
        client
            .batch_execute(
                "GRANT compat_admin_option_role TO compat_admin_option_member WITH ADMIN OPTION",
            )
            .await
            .expect("grant role membership with the admin option");

        // Act
        let granted = client
            .query_one(
                "SELECT membership.admin_option, \
                        pg_has_role(member.rolname, granted_role.rolname, 'MEMBER') \
                 FROM pg_catalog.pg_auth_members AS membership \
                 JOIN pg_catalog.pg_roles AS granted_role \
                   ON granted_role.oid = membership.roleid \
                 JOIN pg_catalog.pg_roles AS member \
                   ON member.oid = membership.member \
                 WHERE granted_role.rolname = 'compat_admin_option_role' \
                   AND member.rolname = 'compat_admin_option_member'",
                &[],
            )
            .await
            .expect("read the granted membership and admin option");
        client
            .batch_execute(
                "REVOKE ADMIN OPTION FOR compat_admin_option_role \
                 FROM compat_admin_option_member",
            )
            .await
            .expect("revoke only the membership admin option");
        let revoked = client
            .query_one(
                "SELECT membership.admin_option, \
                        pg_has_role(member.rolname, granted_role.rolname, 'MEMBER') \
                 FROM pg_catalog.pg_auth_members AS membership \
                 JOIN pg_catalog.pg_roles AS granted_role \
                   ON granted_role.oid = membership.roleid \
                 JOIN pg_catalog.pg_roles AS member \
                   ON member.oid = membership.member \
                 WHERE granted_role.rolname = 'compat_admin_option_role' \
                   AND member.rolname = 'compat_admin_option_member'",
                &[],
            )
            .await
            .expect("read membership after revoking the admin option");

        // Assert
        assert_eq!(
            (
                granted.get::<_, bool>(0),
                granted.get::<_, bool>(1),
                revoked.get::<_, bool>(0),
                revoked.get::<_, bool>(1),
            ),
            (true, true, false, true),
            "revoking ADMIN OPTION preserves the role membership",
        );

        drop(client);
        server.shutdown(connection).await;
    });
}
