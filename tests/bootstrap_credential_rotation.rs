use cassie::app::{Cassie, CassieError};
use cassie::config::CassieRuntimeConfig;
#[path = "support/sql.rs"]
mod support;
use support::*;

fn config(password: &str) -> CassieRuntimeConfig {
    CassieRuntimeConfig {
        password: password.to_string(),
        ..CassieRuntimeConfig::default()
    }
}

#[test]
fn should_make_the_configured_bootstrap_password_authoritative_after_restart() {
    // Arrange
    std::env::set_var("CASSIE_STORAGE_MODE", "local");
    let path = data_dir("restart");
    {
        let cassie = Cassie::new_with_data_dir_and_config(&path, config("old-secret"))
            .expect("initial cassie");
        cassie.startup().expect("initial startup");
        cassie.shutdown();
    }

    // Act
    let restarted = Cassie::new_with_data_dir_and_config(&path, config("new-secret"))
        .expect("restarted cassie");
    restarted.startup().expect("restarted startup");
    let old_password = restarted.authenticate_role("root", Some("old-secret"), None);
    let new_password = restarted.authenticate_role("root", Some("new-secret"), None);

    // Assert
    assert!(matches!(old_password, Err(CassieError::Unauthorized)));
    assert!(new_password.is_ok());
    restarted.shutdown();
    let _ = std::fs::remove_dir_all(path);
}
