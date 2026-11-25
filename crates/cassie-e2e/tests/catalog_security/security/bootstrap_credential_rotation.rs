use super::support_sql as support;
use cassie::app::{Cassie, CassieError};
use cassie::config::CassieRuntimeConfig;
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
    let old_password = uuid::Uuid::new_v4().to_string();
    let new_password = uuid::Uuid::new_v4().to_string();
    std::env::set_var("CASSIE_STORAGE_MODE", "local");
    let path = data_dir("restart");
    {
        let cassie = Cassie::new_with_data_dir_and_config(&path, config(&old_password))
            .expect("initial cassie");
        cassie.startup().expect("initial startup");
        cassie.shutdown();
    }

    // Act
    let restarted = Cassie::new_with_data_dir_and_config(&path, config(&new_password))
        .expect("restarted cassie");
    restarted.startup().expect("restarted startup");
    let old_password_result = restarted.authenticate_role("root", Some(&old_password), None);
    let new_password_result = restarted.authenticate_role("root", Some(&new_password), None);

    // Assert
    assert!(matches!(
        old_password_result,
        Err(CassieError::Unauthorized)
    ));
    assert!(new_password_result.is_ok());
    restarted.shutdown();
    let _ = std::fs::remove_dir_all(path);
}
