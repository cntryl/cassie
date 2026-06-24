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

    for key in parameters.keys() {
        if key.starts_with("_pq_") {
            continue;
        }
        if key == "replication" {
            return Err(format!("unsupported startup option: {key}"));
        }
    }

    Ok(())
}
