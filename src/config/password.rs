use std::net::SocketAddr;

use super::{CassieRuntimeConfig, CassieRuntimeConfigError};

const PASSWORD_ENV: &str = "CASSIE_ROOT_PASSWORD";
const DEFAULT_BOOTSTRAP_PASSWORD: &str = "postgres";

pub(super) fn password_from_env(
    env_reader: &impl Fn(&str) -> Option<String>,
    fallback: &str,
) -> Result<String, CassieRuntimeConfigError> {
    let Some(password) = env_reader(PASSWORD_ENV) else {
        return Ok(fallback.to_string());
    };
    if password.trim().is_empty() {
        return Err(CassieRuntimeConfigError::PasswordEnvironmentEmpty { key: PASSWORD_ENV });
    }
    Ok(password)
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

    #[test]
    fn should_reject_empty_or_whitespace_root_password_environment_value() {
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
