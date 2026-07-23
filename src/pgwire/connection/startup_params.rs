use std::collections::HashMap;

pub(super) fn validate_startup_parameters(
    parameters: &HashMap<String, String>,
) -> Result<(), String> {
    if parameters.get("user").is_some_and(String::is_empty) {
        return Ok(());
    }

    if let Some(value) = parameters.get("user") {
        if value.trim().is_empty() {
            return Err("invalid startup option 'user'".to_string());
        }
    }

    if parameters.get("database").is_some_and(String::is_empty) {
        return Err("invalid startup option 'database'".to_string());
    }

    for (key, value) in parameters {
        if key.starts_with("_pq_") {
            continue;
        }
        if key == "replication" {
            return Err(format!("unsupported startup option: {key}"));
        }
        if matches!(key.as_str(), "application_name" | "client_encoding") {
            crate::app::CassieSession::new("postgres".to_string(), None)
                .set_setting(key, value)
                .map_err(|error| error.to_string())?;
        }
    }

    Ok(())
}

pub(super) fn apply_startup_parameters(
    session: &crate::app::CassieSession,
    parameters: &HashMap<String, String>,
) -> Result<(), crate::app::CassieError> {
    for key in ["application_name", "client_encoding"] {
        if let Some(value) = parameters.get(key) {
            session.set_setting(key, value)?;
        }
    }
    Ok(())
}
