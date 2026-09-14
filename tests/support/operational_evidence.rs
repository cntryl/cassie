pub fn validate_operational_evidence_manifest(
    document: &str,
    expected_commit: &str,
) -> Result<(), String> {
    let manifest: serde_json::Value =
        serde_json::from_str(document).map_err(|error| format!("invalid JSON: {error}"))?;
    let object = manifest
        .as_object()
        .ok_or_else(|| "operational evidence manifest must be an object".to_string())?;

    require_string(object, "schema_version", "cassie-operational-evidence.v1")?;
    require_string(object, "commit", expected_commit)?;

    let run_id = string_field(object, "run_id")?;
    if run_id.is_empty()
        || run_id.len() > 128
        || !run_id.as_bytes()[0].is_ascii_alphanumeric()
        || !run_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err("run_id must be a bounded artifact-safe identifier".to_string());
    }

    let platform = string_field(object, "platform")?;
    let profile = string_field(object, "deployment_profile")?;
    let expected_profile = match platform {
        "linux/amd64" => "native-linux-amd64-disk",
        "linux/arm64" => "native-linux-arm64-disk",
        other => {
            return Err(format!(
                "unsupported operational evidence platform '{other}'"
            ))
        }
    };
    if profile != expected_profile {
        return Err(format!(
            "deployment_profile '{profile}' does not match platform '{platform}'"
        ));
    }

    let digest = string_field(object, "image_digest")?;
    let digest_bytes = digest.as_bytes();
    if digest_bytes.len() != 71
        || !digest.starts_with("sha256:")
        || !digest_bytes[7..]
            .iter()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err("image_digest must be an immutable lowercase sha256 digest".to_string());
    }
    require_string(object, "image_revision", expected_commit)?;

    let shape_only = object
        .get("shape_only")
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| "shape_only must be a boolean".to_string())?;
    let steps = object
        .get("steps")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| "steps must be an object".to_string())?;
    for step in [
        "container",
        "snapshot_restore",
        "projection_repair",
        "failure_injection",
    ] {
        require_step_outcome(steps, step, "success")?;
    }
    let long_outcome = step_outcome(steps, "long_evidence")?;
    if (!shape_only && long_outcome != "success")
        || (shape_only && !matches!(long_outcome, "success" | "skipped"))
    {
        return Err(format!(
            "long_evidence outcome '{long_outcome}' is invalid for shape_only={shape_only}"
        ));
    }

    let elapsed = object
        .get("elapsed_ns")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| "elapsed_ns must be an object".to_string())?;
    for step in [
        "container",
        "snapshot_restore",
        "projection_repair",
        "failure_injection",
    ] {
        let value = elapsed
            .get(step)
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| format!("elapsed_ns.{step} must be an exact positive integer"))?;
        if value == 0 {
            return Err(format!("elapsed_ns.{step} must be positive"));
        }
    }

    Ok(())
}

fn string_field<'a>(
    object: &'a serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Result<&'a str, String> {
    object
        .get(field)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{field} must be a non-empty string"))
}

fn require_string(
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
    expected: &str,
) -> Result<(), String> {
    let actual = string_field(object, field)?;
    if actual != expected {
        return Err(format!(
            "{field} expected '{expected}', received '{actual}'"
        ));
    }
    Ok(())
}

fn step_outcome<'a>(
    steps: &'a serde_json::Map<String, serde_json::Value>,
    step: &str,
) -> Result<&'a str, String> {
    steps
        .get(step)
        .and_then(serde_json::Value::as_object)
        .and_then(|value| value.get("outcome"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| format!("steps.{step}.outcome must be a string"))
}

fn require_step_outcome(
    steps: &serde_json::Map<String, serde_json::Value>,
    step: &str,
    expected: &str,
) -> Result<(), String> {
    let actual = step_outcome(steps, step)?;
    if actual != expected {
        return Err(format!(
            "steps.{step}.outcome expected '{expected}', received '{actual}'"
        ));
    }
    Ok(())
}
