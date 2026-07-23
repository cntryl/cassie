use cassie::app::{Cassie, CassieError};
use cassie::config::CassieRuntimeConfig;
use uuid::Uuid;

fn data_dir(label: &str) -> String {
    std::env::temp_dir()
        .join(format!(
            "cassie-bootstrap-credential-{label}-{}",
            Uuid::new_v4()
        ))
        .to_string_lossy()
        .into_owned()
}

fn config(password: &str) -> CassieRuntimeConfig {
    CassieRuntimeConfig {
        password: password.to_string(),
        ..CassieRuntimeConfig::default()
    }
}

#[test]
fn should_make_the_configured_bootstrap_password_authoritative_after_restart() {
    // Arrange
    std::env::set_var("CASSIE_MIDGE_ALLOW_FALLBACK", "1");
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
    let old_password = restarted.authenticate_role("postgres", Some("old-secret"), None);
    let new_password = restarted.authenticate_role("postgres", Some("new-secret"), None);

    // Assert
    assert!(matches!(old_password, Err(CassieError::Unauthorized)));
    assert!(new_password.is_ok());
    restarted.shutdown();
    let _ = std::fs::remove_dir_all(path);
}
