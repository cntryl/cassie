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

/// The parts of a manifest that must match across a bundle for its timings
/// to be comparable. Commit and deployment profile are checked separately
/// against caller-supplied expectations.
#[derive(Debug, PartialEq, Eq)]
struct BundleIdentity {
    fixture: String,
    midge_lock_checksum: String,
    rust_toolchain: serde_json::Value,
    runtime_config: serde_json::Value,
    image_digest: String,
    platform: String,
    core_count: u64,
    memory_total_bytes: u64,
    filesystem: String,
}

impl BundleIdentity {
    fn read(object: &serde_json::Map<String, serde_json::Value>) -> Result<Self, String> {
        let host = object
            .get("host")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| "host must be an object".to_string())?;
        Ok(Self {
            fixture: string_field(object, "fixture")?.to_string(),
            midge_lock_checksum: string_field(object, "midge_lock_checksum")?.to_string(),
            rust_toolchain: object
                .get("rust_toolchain")
                .ok_or_else(|| "rust_toolchain is missing".to_string())?
                .clone(),
            runtime_config: object
                .get("runtime_config")
                .ok_or_else(|| "runtime_config is missing".to_string())?
                .clone(),
            image_digest: string_field(object, "image_digest")?.to_string(),
            platform: string_field(object, "platform")?.to_string(),
            core_count: positive_u64_field(host, "core_count")?,
            memory_total_bytes: positive_u64_field(host, "memory_total_bytes")?,
            filesystem: string_field(host, "filesystem")?.to_string(),
        })
    }

    /// Names the first field that differs, for an error a reader can act on.
    fn difference(&self, other: &Self) -> Option<&'static str> {
        if self.fixture != other.fixture {
            return Some("fixture");
        }
        if self.midge_lock_checksum != other.midge_lock_checksum {
            return Some("midge_lock_checksum");
        }
        if self.rust_toolchain != other.rust_toolchain {
            return Some("rust_toolchain");
        }
        if self.runtime_config != other.runtime_config {
            return Some("runtime_config");
        }
        if self.image_digest != other.image_digest {
            return Some("image_digest");
        }
        if self.platform != other.platform {
            return Some("platform");
        }
        if self.core_count != other.core_count {
            return Some("host.core_count");
        }
        if self.memory_total_bytes != other.memory_total_bytes {
            return Some("host.memory_total_bytes");
        }
        if self.filesystem != other.filesystem {
            return Some("host.filesystem");
        }
        None
    }
}

/// Rejects a repeated run and a `shape_only` run, the two ways a bundle can
/// meet its sample count without holding that many comparable measurements.
fn check_distinct_full_run(
    object: &serde_json::Map<String, serde_json::Value>,
    seen_run_ids: &mut std::collections::BTreeSet<String>,
) -> Result<(), String> {
    let run_id = string_field(object, "run_id")?;
    if !seen_run_ids.insert(run_id.to_string()) {
        return Err(format!(
            "run_id '{run_id}' is repeated; a bundle must hold distinct runs"
        ));
    }
    let shape_only = object
        .get("shape_only")
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| "shape_only must be a boolean".to_string())?;
    if shape_only {
        return Err(
            "shape_only runs prove workflow shape only and cannot support a threshold".to_string(),
        );
    }
    Ok(())
}

/// Validates that a set of retained operational evidence manifests is
/// comparable enough to support a production threshold proposal, and
/// summarizes elapsed-time variance across the bundle.
///
/// Fails closed when the bundle is incomplete (fewer than `min_samples`),
/// empty, any individual manifest is malformed or fails shape validation,
/// any manifest is not pinned to `expected_commit` (mixed-revision), or any
/// manifest's `deployment_profile` does not match `expected_profile`
/// (outside the declared profile).
///
/// Comparability is enforced beyond commit and profile, because a mean over
/// runs that measured different work is not evidence:
///
/// - `run_id` must be unique across the bundle. Otherwise the same retained
///   manifest submitted `min_samples` times satisfies the sample count and
///   reports zero variance, which reads as perfect reproducibility while
///   resting on one measurement.
/// - `shape_only` runs are rejected. They skip the scale and soak owners and
///   are documented as proving workflow shape only, so their timings do not
///   measure the same work as a full run.
/// - `fixture`, `midge_lock_checksum`, `image_digest`, `platform` and the
///   host's `core_count`, `memory_total_bytes` and `filesystem` must be
///   identical across samples, matching the "matching commit, toolchain,
///   fixture, and profile identity" requirement in
///   `docs/capacity-management.md`.
/// - Every sample must report the same `elapsed_ns` metric keys, so a
///   returned metric's `sample_count` is always the bundle size rather than
///   a single run hiding among fully-sampled metrics.
///
/// `stdev_ns` is the sample standard deviation (Bessel-corrected), because
/// these are samples of a larger population of possible runs and the
/// population estimator would understate the spread a threshold must clear.
pub fn validate_threshold_evidence_bundle(
    documents: &[String],
    expected_commit: &str,
    expected_profile: &str,
    min_samples: usize,
) -> Result<std::collections::BTreeMap<String, ThresholdMetricStats>, String> {
    if documents.is_empty() {
        return Err("threshold evidence bundle is empty".to_string());
    }
    if documents.len() < min_samples {
        return Err(format!(
            "threshold evidence bundle has {} sample(s), fewer than the required minimum {min_samples}",
            documents.len()
        ));
    }

    let mut per_metric: std::collections::BTreeMap<String, Vec<f64>> =
        std::collections::BTreeMap::new();
    let mut seen_run_ids = std::collections::BTreeSet::new();
    let mut bundle_identity: Option<BundleIdentity> = None;
    let mut bundle_metrics: Option<std::collections::BTreeSet<String>> = None;

    for (index, document) in documents.iter().enumerate() {
        validate_operational_evidence_manifest(document, expected_commit)
            .map_err(|error| format!("bundle sample {index}: {error}"))?;

        let manifest: serde_json::Value = serde_json::from_str(document)
            .map_err(|error| format!("bundle sample {index}: invalid JSON: {error}"))?;
        let object = manifest
            .as_object()
            .ok_or_else(|| format!("bundle sample {index}: manifest must be an object"))?;
        require_string(object, "schema_version", "cassie-operational-evidence.v2")
            .map_err(|error| format!("bundle sample {index}: {error}; v1 is diagnostic only"))?;

        let profile = string_field(object, "deployment_profile")
            .map_err(|error| format!("bundle sample {index}: {error}"))?;
        if profile != expected_profile {
            return Err(format!(
                "bundle sample {index}: deployment_profile '{profile}' is outside the declared profile '{expected_profile}'"
            ));
        }

        check_distinct_full_run(object, &mut seen_run_ids)
            .map_err(|error| format!("bundle sample {index}: {error}"))?;

        let identity = BundleIdentity::read(object)
            .map_err(|error| format!("bundle sample {index}: {error}"))?;
        match &bundle_identity {
            None => bundle_identity = Some(identity),
            Some(expected) => {
                if let Some(difference) = expected.difference(&identity) {
                    return Err(format!(
                        "bundle sample {index}: {difference} differs from the first sample; samples must be comparable"
                    ));
                }
            }
        }

        let elapsed = object
            .get("elapsed_ns")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| format!("bundle sample {index}: elapsed_ns must be an object"))?;
        let metrics = elapsed
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        match &bundle_metrics {
            None => bundle_metrics = Some(metrics),
            Some(expected) => {
                if *expected != metrics {
                    return Err(format!(
                        "bundle sample {index}: elapsed_ns metrics differ from the first sample; every sample must report the same metrics"
                    ));
                }
            }
        }
        for (metric, value) in elapsed {
            let value = value.as_u64().filter(|value| *value > 0).ok_or_else(|| {
                format!(
                    "bundle sample {index}: elapsed_ns.{metric} must be an exact positive integer"
                )
            })?;
            let value_ns = u64_to_f64(value);
            per_metric.entry(metric.clone()).or_default().push(value_ns);
        }
    }

    Ok(per_metric
        .into_iter()
        .map(|(metric, samples)| (metric, summarize_metric(&samples)))
        .collect())
}

/// Converts without a lossy `as` cast: each 32-bit half is exact in `f64`,
/// so the only rounding is the single final addition.
fn u64_to_f64(value: u64) -> f64 {
    let high = u32::try_from(value >> 32).unwrap_or(u32::MAX);
    let low = u32::try_from(value & u64::from(u32::MAX)).unwrap_or(u32::MAX);
    f64::from(high) * 4_294_967_296.0 + f64::from(low)
}

fn summarize_metric(samples: &[f64]) -> ThresholdMetricStats {
    let sample_count = samples.len();
    let sample_count_f64 = u64_to_f64(u64::try_from(sample_count).unwrap_or(u64::MAX));
    let mean_ns = samples.iter().sum::<f64>() / sample_count_f64;
    // Bessel-corrected: these are samples of the population of possible
    // runs, and dividing by n would bias the spread low, toward a
    // tighter-looking threshold.
    let variance = if sample_count > 1 {
        samples
            .iter()
            .map(|value| (value - mean_ns).powi(2))
            .sum::<f64>()
            / (sample_count_f64 - 1.0)
    } else {
        0.0
    };
    ThresholdMetricStats {
        sample_count,
        mean_ns,
        stdev_ns: variance.sqrt(),
    }
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

    validate_schema_and_identity(object)?;
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

/// The `rust_toolchain` fields a v2 manifest must carry.
///
/// The workflow that emits manifests and the validator that reads them are
/// separate files; `should_emit_every_v2_identity_field_from_the_workflow`
/// asserts the emitter against these same names so neither can drift.
pub const V2_TOOLCHAIN_FIELDS: &[&str] = &["rustc_verbose", "cargo_version"];

/// The `runtime_config` fields a v2 manifest must carry.
pub const V2_RUNTIME_CONFIG_FIELDS: &[&str] = &[
    "storage_mode",
    "storage_path_kind",
    "rest_transport",
    "benchmark_profile",
    "query_timeout_ms",
    "embeddings_provider",
    "soak_duration_seconds",
];

fn validate_schema_and_identity(
    object: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    match string_field(object, "schema_version")? {
        "cassie-operational-evidence.v1" => Ok(()),
        "cassie-operational-evidence.v2" => validate_v2_identity(object),
        other => Err(format!("unsupported schema_version '{other}'")),
    }
}

fn validate_v2_identity(object: &serde_json::Map<String, serde_json::Value>) -> Result<(), String> {
    let toolchain = identity_fields(object, "rust_toolchain", V2_TOOLCHAIN_FIELDS)?;
    let rustc = string_field(toolchain, "rustc_verbose")
        .map_err(|error| format!("rust_toolchain.{error}"))?;
    if !rustc.starts_with("rustc ")
        || !rustc.contains("\nrelease: ")
        || !rustc.contains("\ncommit-hash: ")
        || !rustc.contains("\nhost: ")
    {
        return Err(
            "rust_toolchain.rustc_verbose must contain the complete compiler identity".to_string(),
        );
    }
    validate_toolchain_host(object, rustc)?;
    let cargo = string_field(toolchain, "cargo_version")
        .map_err(|error| format!("rust_toolchain.{error}"))?;
    if !cargo.starts_with("cargo ") {
        return Err("rust_toolchain.cargo_version must identify Cargo".to_string());
    }

    let config = identity_fields(object, "runtime_config", V2_RUNTIME_CONFIG_FIELDS)?;
    for (field, expected) in [
        ("storage_mode", "local"),
        ("storage_path_kind", "isolated-local-disk"),
        ("rest_transport", "private-hop-http"),
        ("benchmark_profile", "release"),
        ("embeddings_provider", "disabled"),
    ] {
        require_string(config, field, expected)
            .map_err(|error| format!("runtime_config.{error}"))?;
    }
    validate_soak_duration(object, config)?;
    positive_u64_field(config, "query_timeout_ms")
        .map_err(|error| format!("runtime_config.{error}"))?;
    Ok(())
}

/// A shape-only run skips the soak owners entirely, so the only honest
/// duration it can record is zero. Accepting any positive number here is what
/// let a skipped soak be retained as a completed one-hour run.
fn validate_soak_duration(
    manifest: &serde_json::Map<String, serde_json::Value>,
    config: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    let shape_only = manifest
        .get("shape_only")
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| "shape_only must be a boolean".to_string())?;
    let seconds = config
        .get("soak_duration_seconds")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| {
            "runtime_config.soak_duration_seconds must be an exact non-negative integer".to_string()
        })?;
    match (shape_only, seconds) {
        (true, 0) => Ok(()),
        (true, _) => Err(format!(
            "runtime_config.soak_duration_seconds must be 0 when shape_only is true, found {seconds}"
        )),
        (false, 0) => {
            Err(
                "runtime_config.soak_duration_seconds must be positive when shape_only is false"
                    .to_string(),
            )
        }
        (false, _) => Ok(()),
    }
}

fn validate_toolchain_host(
    object: &serde_json::Map<String, serde_json::Value>,
    rustc_verbose: &str,
) -> Result<(), String> {
    let expected_host = match string_field(object, "platform")? {
        "linux/amd64" => "x86_64-unknown-linux-gnu",
        "linux/arm64" => "aarch64-unknown-linux-gnu",
        other => {
            return Err(format!(
                "unsupported operational evidence platform '{other}'"
            ))
        }
    };
    if !rustc_verbose
        .lines()
        .any(|line| line == format!("host: {expected_host}"))
    {
        return Err("rust_toolchain host does not match platform".to_string());
    }
    Ok(())
}

fn identity_fields<'a>(
    object: &'a serde_json::Map<String, serde_json::Value>,
    name: &str,
    expected: &[&str],
) -> Result<&'a serde_json::Map<String, serde_json::Value>, String> {
    let fields = object
        .get(name)
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| format!("{name} must be an object"))?;
    if fields.len() != expected.len()
        || fields
            .keys()
            .any(|field| !expected.contains(&field.as_str()))
    {
        return Err(format!(
            "{name} must contain only the normalized, secret-free identity fields"
        ));
    }
    Ok(fields)
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
