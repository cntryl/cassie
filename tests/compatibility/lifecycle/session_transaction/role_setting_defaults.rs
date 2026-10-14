use super::*;

#[test]
fn should_restore_role_setting_default_after_session_reset() {
    // Arrange
    let runtime = current_thread_runtime();
    runtime.block_on(async {
        let server = CompatibilityServer::start("role_setting_default").await;
        let (existing_session, existing_connection) = server.connect().await;
        let startup_setting = existing_session
            .query_one("SHOW application_name", &[])
            .await;
        let set_role_default = existing_session
            .batch_execute(
                "ALTER ROLE CURRENT_USER SET application_name TO 'compat_role_default_821'",
            )
            .await;

        // Act
        let mut new_connection = None;
        let mut new_session_observations = None;
        let mut role_default_cleanup = None;
        let existing_setting_after_alter = if set_role_default.is_ok() {
            let current_setting = existing_session
                .query_one("SHOW application_name", &[])
                .await;
            let (new_session, connection) = server.connect().await;
            new_connection = Some(connection);
            let setting_on_login = new_session.query_one("SHOW application_name", &[]).await;
            let set_session_override = new_session
                .batch_execute("SET application_name = 'compat_session_override_821'")
                .await;
            let setting_after_override = if set_session_override.is_ok() {
                Some(new_session.query_one("SHOW application_name", &[]).await)
            } else {
                None
            };
            let reset_session_override = if set_session_override.is_ok() {
                Some(new_session.batch_execute("RESET application_name").await)
            } else {
                None
            };
            let setting_after_reset = if reset_session_override.as_ref().is_some_and(Result::is_ok)
            {
                Some(new_session.query_one("SHOW application_name", &[]).await)
            } else {
                None
            };
            role_default_cleanup = Some(
                new_session
                    .batch_execute("ALTER ROLE CURRENT_USER RESET application_name")
                    .await,
            );
            new_session_observations = Some((
                setting_on_login,
                set_session_override,
                setting_after_override,
                reset_session_override,
                setting_after_reset,
            ));
            Some(current_setting)
        } else {
            None
        };

        drop(existing_session);
        if let Some(connection) = new_connection {
            connection.abort();
            let _ = connection.await;
        }
        server.shutdown(existing_connection).await;

        // Assert
        assert!(
            set_role_default.is_ok(),
            "ALTER ROLE should save a session default: {set_role_default:?}"
        );
        let startup_setting = startup_setting.expect("read the existing session setting");
        let startup_value: String = startup_setting
            .try_get(0)
            .expect("application_name startup value");
        assert_eq!(startup_setting.columns()[0].type_().oid(), 25);

        let current_setting = existing_setting_after_alter
            .expect("read application_name after changing the role default")
            .expect("read the still-connected session setting");
        assert_eq!(current_setting.columns()[0].type_().oid(), 25);
        assert_eq!(
            current_setting
                .try_get::<_, String>(0)
                .expect("current session application_name"),
            startup_value,
            "ALTER ROLE changes future logins without changing the current session"
        );

        let (
            setting_on_login,
            set_session_override,
            setting_after_override,
            reset_session_override,
            setting_after_reset,
        ) = new_session_observations.expect("observe the newly connected role session");
        let setting_on_login = setting_on_login.expect("read application_name at login");
        assert_eq!(setting_on_login.columns()[0].type_().oid(), 25);
        assert_eq!(
            setting_on_login
                .try_get::<_, String>(0)
                .expect("role default application_name"),
            "compat_role_default_821"
        );
        set_session_override.expect("set a session-local application_name override");
        let setting_after_override = setting_after_override
            .expect("read the overridden application_name")
            .expect("SHOW application_name after SET");
        assert_eq!(setting_after_override.columns()[0].type_().oid(), 25);
        assert_eq!(
            setting_after_override
                .try_get::<_, String>(0)
                .expect("overridden application_name"),
            "compat_session_override_821"
        );
        reset_session_override
            .expect("RESET should be attempted after setting the override")
            .expect("RESET application_name");
        let setting_after_reset = setting_after_reset
            .expect("read application_name after RESET")
            .expect("SHOW application_name after RESET");
        assert_eq!(setting_after_reset.columns()[0].type_().oid(), 25);
        assert_eq!(
            setting_after_reset
                .try_get::<_, String>(0)
                .expect("reset application_name"),
            "compat_role_default_821",
            "RESET restores the per-role login default"
        );
        role_default_cleanup
            .expect("role-default cleanup should be attempted")
            .expect("remove the role default before closing the server");
    });
}
