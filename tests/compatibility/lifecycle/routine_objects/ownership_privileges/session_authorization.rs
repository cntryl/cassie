use super::*;

#[test]
fn should_restore_session_identity_after_authorization_changes() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("session_authorization_identity").await;
        let (client, connection) = server.connect().await;
        client
            .batch_execute("CREATE ROLE compat_session_auth_target NOLOGIN")
            .await
            .expect("create a role to use as the alternate session authorization");
        let original_identity = client
            .query_one("SELECT current_user, session_user", &[])
            .await
            .expect("read the connected session identities");
        let original_current_user = original_identity.get::<_, String>(0);
        let original_session_user = original_identity.get::<_, String>(1);

        // Act
        client
            .batch_execute("SET SESSION AUTHORIZATION compat_session_auth_target")
            .await
            .expect("set the session authorization to the target role");
        let changed_identity = client
            .query_one("SELECT current_user, session_user", &[])
            .await
            .expect("read both identities after SET SESSION AUTHORIZATION");
        let missing_role_error = client
            .batch_execute("SET SESSION AUTHORIZATION compat_session_auth_missing")
            .await
            .expect_err("reject a session authorization role that does not exist");
        let identity_after_error = client
            .query_one("SELECT current_user, session_user", &[])
            .await
            .expect("read identities after the rejected authorization change");
        client
            .batch_execute("SET SESSION AUTHORIZATION DEFAULT")
            .await
            .expect("restore the initial session identity with DEFAULT");
        let identity_after_default = client
            .query_one("SELECT current_user, session_user", &[])
            .await
            .expect("read identities after SET SESSION AUTHORIZATION DEFAULT");
        client
            .batch_execute("SET SESSION AUTHORIZATION compat_session_auth_target")
            .await
            .expect("set the alternate session identity a second time");
        let changed_again = client
            .query_one("SELECT current_user, session_user", &[])
            .await
            .expect("read identities before RESET SESSION AUTHORIZATION");
        client
            .batch_execute("RESET SESSION AUTHORIZATION")
            .await
            .expect("restore the initial session identity with RESET");
        let identity_after_reset = client
            .query_one("SELECT current_user, session_user", &[])
            .await
            .expect("read identities after RESET SESSION AUTHORIZATION");

        // Assert
        assert_eq!(
            (
                changed_identity.get::<_, String>(0),
                changed_identity.get::<_, String>(1),
            ),
            (
                "compat_session_auth_target".to_owned(),
                "compat_session_auth_target".to_owned(),
            )
        );
        assert_eq!(db_error(&missing_role_error).code().code(), "22023");
        assert_eq!(
            (
                identity_after_error.get::<_, String>(0),
                identity_after_error.get::<_, String>(1),
            ),
            (
                "compat_session_auth_target".to_owned(),
                "compat_session_auth_target".to_owned(),
            ),
            "a failed role change must preserve the current authorization",
        );
        for restored in [&identity_after_default, &identity_after_reset] {
            assert_eq!(
                (restored.get::<_, String>(0), restored.get::<_, String>(1),),
                (original_current_user.clone(), original_session_user.clone(),)
            );
        }
        assert_eq!(
            (
                changed_again.get::<_, String>(0),
                changed_again.get::<_, String>(1),
            ),
            (
                "compat_session_auth_target".to_owned(),
                "compat_session_auth_target".to_owned(),
            )
        );

        client
            .batch_execute("DROP ROLE compat_session_auth_target")
            .await
            .expect("drop the temporary session authorization role");
        drop(client);
        server.shutdown(connection).await;
    });
}
