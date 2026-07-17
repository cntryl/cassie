use std::net::SocketAddr;

use super::{CassieRuntimeConfig, CassieRuntimeConfigError};

const PASSWORD_ENV: &str = "CASSIE_ADMIN_PASSWORD";
const PASSWORD_FILE_ENV: &str = "CASSIE_ADMIN_PASSWORD_FILE";
const DEFAULT_BOOTSTRAP_PASSWORD: &str = "postgres";

pub(super) fn password_from_env(
    env_reader: &impl Fn(&str) -> Option<String>,
    fallback: &str,
) -> Result<String, CassieRuntimeConfigError> {
    if let Some(path) = env_reader(PASSWORD_FILE_ENV) {
        return read_password_file(path);
    }
    let Some(password) = env_reader(PASSWORD_ENV) else {
        return Ok(fallback.to_string());
    };
    if password.trim().is_empty() {
        return Err(CassieRuntimeConfigError::PasswordEnvironmentEmpty { key: PASSWORD_ENV });
    }
    Ok(password)
}

fn read_password_file(path: String) -> Result<String, CassieRuntimeConfigError> {
    let value = std::fs::read_to_string(&path).map_err(|source| {
        CassieRuntimeConfigError::PasswordFileRead {
            key: PASSWORD_FILE_ENV,
            path: path.clone(),
            source,
        }
    })?;
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(CassieRuntimeConfigError::PasswordFileEmpty {
            key: PASSWORD_FILE_ENV,
            path,
        });
    }
    Ok(value)
}

pub(super) fn validate_bootstrap_password(
    config: &CassieRuntimeConfig,
) -> Result<(), CassieRuntimeConfigError> {
    for listener in [&config.pgwire_listen, &config.rest_listen] {
        let Ok(address) = listener.parse::<SocketAddr>() else {
            continue;
        };
        validate_listener_password(&config.password, address)?;
    }
    Ok(())
}

pub(crate) fn validate_listener_password(
    password: &str,
    listener: SocketAddr,
) -> Result<(), CassieRuntimeConfigError> {
    if password.trim().is_empty() {
        return Err(CassieRuntimeConfigError::EmptyBootstrapPassword {
            listener: listener.to_string(),
        });
    }
    if password == DEFAULT_BOOTSTRAP_PASSWORD && !listener.ip().is_loopback() {
        return Err(CassieRuntimeConfigError::UnsafeDefaultPassword {
            listener: listener.to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn env_reader(values: HashMap<&'static str, String>) -> impl Fn(&str) -> Option<String> {
        move |key| values.get(key).cloned()
    }

    fn temp_file(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("cassie-config-{label}-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn should_use_admin_password_file_before_admin_password_environment() {
        // Arrange
        let path = temp_file("password-file-precedence");
        std::fs::write(&path, " file-secret \n").expect("write password file");
        let values = HashMap::from([
            (PASSWORD_FILE_ENV, path.to_string_lossy().to_string()),
            (PASSWORD_ENV, "env-secret".to_string()),
        ]);

        // Act
        let password = password_from_env(&env_reader(values), DEFAULT_BOOTSTRAP_PASSWORD)
            .expect("configured password");

        // Assert
        assert_eq!(password, "file-secret");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn should_reject_missing_admin_password_file_without_environment_fallback() {
        // Arrange
        let path = temp_file("missing-password-file");
        let values = HashMap::from([
            (PASSWORD_FILE_ENV, path.to_string_lossy().to_string()),
            (PASSWORD_ENV, "env-secret".to_string()),
        ]);

        // Act
        let error = password_from_env(&env_reader(values), DEFAULT_BOOTSTRAP_PASSWORD)
            .expect_err("missing password file should fail");

        // Assert
        assert!(error.to_string().contains(PASSWORD_FILE_ENV));
        assert!(error.to_string().contains(path.to_string_lossy().as_ref()));
    }

    #[test]
    fn should_reject_empty_admin_password_file_without_environment_fallback() {
        // Arrange
        let path = temp_file("empty-password-file");
        std::fs::write(&path, " \n\t").expect("write password file");
        let values = HashMap::from([
            (PASSWORD_FILE_ENV, path.to_string_lossy().to_string()),
            (PASSWORD_ENV, "env-secret".to_string()),
        ]);

        // Act
        let error = password_from_env(&env_reader(values), DEFAULT_BOOTSTRAP_PASSWORD)
            .expect_err("empty password file should fail");

        // Assert
        assert!(error.to_string().contains("empty"));
        assert!(error.to_string().contains(path.to_string_lossy().as_ref()));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn should_reject_empty_or_whitespace_admin_password_environment_value() {
        // Arrange
        let values = ["", " \n\t"];

        // Act
        let errors = values.map(|value| {
            password_from_env(
                &env_reader(HashMap::from([(PASSWORD_ENV, value.to_string())])),
                DEFAULT_BOOTSTRAP_PASSWORD,
            )
            .expect_err("empty environment password should fail")
        });

        // Assert
        for error in errors {
            assert!(error.to_string().contains(PASSWORD_ENV));
            assert!(error.to_string().contains("empty"));
        }
    }
}
