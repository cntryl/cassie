use super::*;

struct SessionAuthorizationObservations<'a> {
    original_current_user: &'a str,
    original_session_user: &'a str,
    changed: &'a tokio_postgres::Row,
    missing_role_error: &'a tokio_postgres::Error,
    after_error: &'a tokio_postgres::Row,
    after_default: &'a tokio_postgres::Row,
    changed_again: &'a tokio_postgres::Row,
    after_reset: &'a tokio_postgres::Row,
}

fn assert_session_authorization_transitions(observations: &SessionAuthorizationObservations<'_>) {
    let target = ("compat_session_auth_target", "compat_session_auth_target");
    for identity in [
        observations.changed,
        observations.after_error,
        observations.changed_again,
    ] {
        assert_eq!(
            (identity.get::<_, String>(0), identity.get::<_, String>(1),),
            (target.0.to_owned(), target.1.to_owned())
        );
    }
    assert_eq!(
        db_error(observations.missing_role_error).code().code(),
        "22023"
    );
    for restored in [observations.after_default, observations.after_reset] {
        assert_eq!(
            (restored.get::<_, String>(0), restored.get::<_, String>(1),),
            (
                observations.original_current_user.to_owned(),
                observations.original_session_user.to_owned(),
            )
        );
    }
}

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
        assert_session_authorization_transitions(&SessionAuthorizationObservations {
            original_current_user: &original_current_user,
            original_session_user: &original_session_user,
            changed: &changed_identity,
            missing_role_error: &missing_role_error,
            after_error: &identity_after_error,
            after_default: &identity_after_default,
            changed_again: &changed_again,
            after_reset: &identity_after_reset,
        });

        client
            .batch_execute("DROP ROLE compat_session_auth_target")
            .await
            .expect("drop the temporary session authorization role");
        drop(client);
        server.shutdown(connection).await;
    });
}
