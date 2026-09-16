/// Per-metric sample statistics computed across a comparable evidence bundle.
/// `mean_ns` and `stdev_ns` support the "record sample count and observed
/// variance" requirement for proposing a production threshold; they are not
/// themselves a threshold or SLA.
#[derive(Debug, Clone, PartialEq)]
pub struct ThresholdMetricStats {
    pub sample_count: usize,
    pub mean_ns: f64,
    pub stdev_ns: f64,
}

/// Validates that a set of retained operational evidence manifests is
/// comparable enough to support a production threshold proposal, and
/// summarizes elapsed-time variance across the bundle.
///
/// Fails closed when the bundle is incomplete (fewer than `min_samples`),
/// any individual manifest is malformed or fails shape validation, any
/// manifest is not pinned to `expected_commit` (mixed-revision), or any
/// manifest's `deployment_profile` does not match `expected_profile`
/// (outside the declared profile). A single measurement can never satisfy
/// `min_samples > 1`, so it can never be promoted to an SLA by this
/// function alone.
pub fn validate_threshold_evidence_bundle(
    documents: &[String],
    expected_commit: &str,
    expected_profile: &str,
    min_samples: usize,
) -> Result<std::collections::BTreeMap<String, ThresholdMetricStats>, String> {
    if documents.len() < min_samples {
        return Err(format!(
            "threshold evidence bundle has {} sample(s), fewer than the required minimum {min_samples}",
            documents.len()
        ));
    }

    let mut per_metric: std::collections::BTreeMap<String, Vec<f64>> =
        std::collections::BTreeMap::new();

    for (index, document) in documents.iter().enumerate() {
        validate_operational_evidence_manifest(document, expected_commit)
            .map_err(|error| format!("bundle sample {index}: {error}"))?;

        let manifest: serde_json::Value = serde_json::from_str(document)
            .map_err(|error| format!("bundle sample {index}: invalid JSON: {error}"))?;
        let object = manifest
            .as_object()
            .ok_or_else(|| format!("bundle sample {index}: manifest must be an object"))?;

        let profile = string_field(object, "deployment_profile")
            .map_err(|error| format!("bundle sample {index}: {error}"))?;
        if profile != expected_profile {
            return Err(format!(
                "bundle sample {index}: deployment_profile '{profile}' is outside the declared profile '{expected_profile}'"
            ));
        }

        let elapsed = object
            .get("elapsed_ns")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| format!("bundle sample {index}: elapsed_ns must be an object"))?;
        for (metric, value) in elapsed {
            let value = value.as_u64().ok_or_else(|| {
                format!(
                    "bundle sample {index}: elapsed_ns.{metric} must be an exact positive integer"
                )
            })?;
            #[allow(clippy::cast_precision_loss)]
            let value_ns = value as f64;
            per_metric.entry(metric.clone()).or_default().push(value_ns);
        }
    }

    Ok(per_metric
        .into_iter()
        .map(|(metric, samples)| {
            let sample_count = samples.len();
            #[allow(clippy::cast_precision_loss)]
            let sample_count_f64 = sample_count as f64;
            let mean_ns = samples.iter().sum::<f64>() / sample_count_f64;
            let variance = samples
                .iter()
                .map(|value| (value - mean_ns).powi(2))
                .sum::<f64>()
                / sample_count_f64;
            (
                metric,
                ThresholdMetricStats {
                    sample_count,
                    mean_ns,
                    stdev_ns: variance.sqrt(),
                },
            )
        })
        .collect())
}

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
    for field in ["operator", "runner", "fixture"] {
        string_field(object, field)?;
    }
    validate_lowercase_sha256_field(object, "midge_lock_checksum")?;
    validate_utc_timestamp(object, "started_utc")?;
    validate_utc_timestamp(object, "finished_utc")?;
    if string_field(object, "finished_utc")? < string_field(object, "started_utc")? {
        return Err("finished_utc must not precede started_utc".to_string());
    }

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

    validate_host_resources(object)?;

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
        "container_snapshot_restore",
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

fn validate_host_resources(
    object: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    let host = object
        .get("host")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| "host must be an object".to_string())?;
    for field in ["cpu_model", "filesystem"] {
        string_field(host, field).map_err(|error| format!("host.{error}"))?;
    }
    for field in [
        "core_count",
        "memory_total_bytes",
        "disk_total_bytes",
        "disk_available_bytes",
    ] {
        positive_u64_field(host, field).map_err(|error| format!("host.{error}"))?;
    }
    let total = positive_u64_field(host, "disk_total_bytes")?;
    let available = positive_u64_field(host, "disk_available_bytes")?;
    if available > total {
        return Err("host.disk_available_bytes must not exceed host.disk_total_bytes".to_string());
    }
    Ok(())
}

fn positive_u64_field(
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Result<u64, String> {
    object
        .get(field)
        .and_then(serde_json::Value::as_u64)
        .filter(|value| *value > 0)
        .ok_or_else(|| format!("{field} must be an exact positive integer"))
}

fn validate_lowercase_sha256_field(
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Result<(), String> {
    let value = string_field(object, field)?;
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(format!("{field} must be a lowercase sha256 digest"));
    }
    Ok(())
}

fn validate_utc_timestamp(
    object: &serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Result<(), String> {
    let value = string_field(object, field)?;
    let bytes = value.as_bytes();
    let separators_match = bytes.len() == 20
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[19] == b'Z';
    let digits_match = bytes
        .iter()
        .enumerate()
        .all(|(index, byte)| matches!(index, 4 | 7 | 10 | 13 | 16 | 19) || byte.is_ascii_digit());
    if !separators_match || !digits_match {
        return Err(format!("{field} must be an RFC 3339 UTC second timestamp"));
    }
    time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
        .map_err(|_| format!("{field} must be an RFC 3339 UTC second timestamp"))?;
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
