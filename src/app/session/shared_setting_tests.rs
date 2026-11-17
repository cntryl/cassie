//! Shared setting leaves use their actual root, including detached callers.

use super::*;
use crate::app::Cassie;
use crate::config::CassieRuntimeConfig;

fn fixture() -> (Cassie, CassieSession) {
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    let session = cassie.create_session("setting_owner", None);
    (cassie, session)
}

#[test]
fn should_keep_replacement_settings_and_mode_from_an_old_detached_leaf() {
    for leaf in ["set", "local", "reset", "reset_all", "path", "mode"] {
        // Arrange
        let (cassie, session) = fixture();
        cassie
            .execute_sql(&session, "BEGIN", Vec::new())
            .expect("begin A");
        let authority = FailureAuthority::capture(&session);
        let operation = session.with_operation_failure_authority(Some(&authority));
        let batch = cassie
            .new_statement_batch(Some(&operation))
            .expect("fork A");
        cassie
            .execute_sql(&session, "ROLLBACK", Vec::new())
            .expect("rollback A");
        cassie
            .execute_sql(&session, "BEGIN", Vec::new())
            .expect("begin B");
        session
            .set_setting("application_name", "B")
            .expect("B setting");
        session
            .set_search_path_for_operation(vec!["public".to_owned(), "B".to_owned()])
            .expect("B path");
        session
            .set_projection_write_mode(Some("replayable"))
            .expect("B mode");
        let current = session.transaction_unique_owner_id();
        // Act
        let result = match leaf {
            "set" => batch
                .session()
                .set_setting("application_name", "old")
                .map(|_| ()),
            "local" => batch.session().set_local_setting("application_name", "old"),
            "reset" => batch.session().reset_setting("application_name"),
            "reset_all" => batch.session().reset_all_settings(),
            "path" => batch
                .session()
                .set_search_path_for_operation(vec!["old".to_owned()]),
            "mode" => batch.session().set_projection_write_mode(Some("strict")),
            _ => unreachable!(),
        };
        // Assert
        assert_eq!(result.expect_err(leaf).sql_state(), "25000", "{leaf}");
        assert_eq!(session.transaction_unique_owner_id(), current);
        assert_eq!(session.transaction_status(), "in_transaction");
        assert_eq!(session.setting("application_name").expect("B setting"), "B");
        assert_eq!(session.search_path(), vec!["public", "B"]);
        assert_eq!(
            session
                .setting("cassie.projection_write_mode")
                .expect("B mode"),
            "replayable"
        );
    }
}

#[test]
fn should_record_current_fork_local_history_in_the_actual_root() {
    for terminal in ["COMMIT", "ROLLBACK"] {
        // Arrange
        let (cassie, session) = fixture();
        session
            .set_setting("application_name", "before")
            .expect("initial setting");
        cassie
            .execute_sql(&session, "BEGIN", Vec::new())
            .expect("begin");
        let authority = FailureAuthority::capture(&session);
        let operation = session.with_operation_failure_authority(Some(&authority));
        let batch = cassie
            .new_statement_batch(Some(&operation))
            .expect("current fork");
        // Act
        batch
            .session()
            .set_local_setting("application_name", "local")
            .expect("owned local setting");
        batch
            .session()
            .set_search_path_for_operation(vec!["public".to_owned(), "fork".to_owned()])
            .expect("owned path");
        let visible = session
            .setting("application_name")
            .expect("current local value");
        cassie
            .execute_sql(&session, terminal, Vec::new())
            .expect("retire actual root");
        // Assert
        assert_eq!(visible, "local");
        assert_eq!(
            session.setting("application_name").expect("restored value"),
            "before"
        );
        assert_eq!(
            session.search_path(),
            if terminal == "COMMIT" {
                vec!["public", "fork"]
            } else {
                vec!["public"]
            }
        );
    }
}

#[test]
fn should_preserve_unscoped_public_setting_apis_current_session_meaning() {
    // Arrange
    let (cassie, session) = fixture();
    cassie
        .execute_sql(&session, "BEGIN", Vec::new())
        .expect("begin");
    let seal = session.seal_transaction_for_commit();
    // Act
    session
        .set_setting("application_name", "trusted")
        .expect("current-session API");
    session.set_search_path(vec!["public".to_owned(), "trusted".to_owned()]);
    drop(seal);
    // Assert
    assert_eq!(
        session.setting("application_name").expect("value"),
        "trusted"
    );
    assert_eq!(session.search_path(), vec!["public", "trusted"]);
}
