use std::collections::HashMap;

/// Startup packet keys that are protocol fields rather than settings.
const PROTOCOL_KEYS: &[&str] = &["user", "database", "options", "replication"];

pub(super) fn validate_startup_parameters(
    parameters: &HashMap<String, String>,
) -> Result<(), String> {
    if let Some(value) = parameters.get("user") {
        if !value.is_empty() && value.trim().is_empty() {
            return Err("invalid startup option 'user'".to_string());
        }
    }

    if parameters.get("database").is_some_and(String::is_empty) {
        return Err("invalid startup option 'database'".to_string());
    }

    for key in parameters.keys() {
        if key == "replication" {
            return Err(format!("unsupported startup option: {key}"));
        }
    }
    let probe = crate::app::CassieSession::new("root".to_string(), None);
    for (key, value) in startup_settings(parameters)? {
        probe
            .set_startup_setting(&key, &value)
            .map_err(|error| error.to_string())?;
    }

    Ok(())
}

pub(super) fn apply_startup_parameters(
    session: &crate::app::CassieSession,
    parameters: &HashMap<String, String>,
) -> Result<(), crate::app::CassieError> {
    let settings = startup_settings(parameters).map_err(crate::app::CassieError::Parse)?;
    for (key, value) in settings {
        session.set_startup_setting(&key, &value)?;
    }
    Ok(())
}

/// The settings a startup packet assigns: settings named directly as packet
/// keys, then `-c name=value` and `--name=value` entries of `options`.
fn startup_settings(parameters: &HashMap<String, String>) -> Result<Vec<(String, String)>, String> {
    // Packet keys naming registered mutable settings apply directly; other
    // keys keep their historical tolerance, so drivers that send fixed
    // compatibility values such as `extra_float_digits` still connect.
    let mut settings = parameters
        .iter()
        .filter(|(key, _)| !key.starts_with("_pq_") && !PROTOCOL_KEYS.contains(&key.as_str()))
        .filter(|(key, _)| {
            crate::app::all_session_settings().iter().any(|spec| {
                spec.name.eq_ignore_ascii_case(key)
                    && (spec.kind == crate::app::SettingKind::Mutable
                        || spec.name == "client_encoding")
            })
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<Vec<_>>();
    settings.sort();
    if let Some(options) = parameters.get("options") {
        settings.extend(option_settings(options)?);
    }
    Ok(settings)
}

/// Splits the `options` startup value as PostgreSQL does: arguments are
/// separated by whitespace, and a backslash escapes the next character.
fn option_settings(options: &str) -> Result<Vec<(String, String)>, String> {
    let mut arguments = Vec::new();
    let mut current = String::new();
    let mut characters = options.chars();
    while let Some(character) = characters.next() {
        match character {
            '\\' => current.extend(characters.next()),
            character if character.is_whitespace() => {
                if !current.is_empty() {
                    arguments.push(std::mem::take(&mut current));
                }
            }
            character => current.push(character),
        }
    }
    if !current.is_empty() {
        arguments.push(current);
    }

    let mut settings = Vec::new();
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        let assignment = if argument == "-c" {
            arguments
                .next()
                .ok_or_else(|| "invalid command-line argument: -c".to_string())?
        } else if let Some(assignment) = argument
            .strip_prefix("--")
            .or_else(|| argument.strip_prefix("-c"))
        {
            assignment.to_string()
        } else {
            return Err(format!("invalid command-line argument: {argument}"));
        };
        let Some((name, value)) = assignment.split_once('=') else {
            return Err(format!("-c {assignment} requires a value"));
        };
        settings.push((name.replace('-', "_"), value.to_string()));
    }
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::option_settings;

    #[test]
    fn should_split_startup_options_given_c_and_long_forms_when_options_are_read() {
        // Arrange
        let options = r"-c statement_timeout=1234ms --lock-timeout=5s -capplication_name=a\ b";

        // Act
        let settings = option_settings(options).expect("startup options");

        // Assert
        assert_eq!(
            settings,
            [
                ("statement_timeout".to_string(), "1234ms".to_string()),
                ("lock_timeout".to_string(), "5s".to_string()),
                ("application_name".to_string(), "a b".to_string()),
            ]
        );
    }

    #[test]
    fn should_reject_startup_options_given_a_bare_argument_when_options_are_read() {
        // Arrange
        let options = "statement_timeout=1";

        // Act
        let rejected = option_settings(options);

        // Assert
        assert!(rejected.is_err());
    }
}
