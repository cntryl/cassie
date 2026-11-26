use std::time::Duration;

const DEFAULT_SOAK_DURATION_SECONDS: &str = "3600";
const MINIMUM_CANONICAL_SOAK_DURATION: Duration = Duration::from_hours(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SoakDuration {
    pub total: Duration,
    pub source: &'static str,
}

/// # Errors
///
/// Returns an error when the selected duration is not a positive integer.
pub fn resolve_soak_duration(
    environment: Option<&str>,
    command_line: Option<&str>,
) -> Result<SoakDuration, String> {
    let (raw, source) = command_line
        .map(|value| (value, "cli"))
        .or_else(|| environment.map(|value| (value, "environment")))
        .unwrap_or((DEFAULT_SOAK_DURATION_SECONDS, "default"));
    let seconds = raw
        .parse::<u64>()
        .map_err(|_| format!("soak duration must be a positive integer, got '{raw}'"))?;
    if seconds == 0 {
        return Err("soak duration must be positive".to_string());
    }
    Ok(SoakDuration {
        total: Duration::from_secs(seconds),
        source,
    })
}

/// Rejects shortened endurance evidence outside the explicitly diagnostic smoke profile.
///
/// # Errors
///
/// Returns an error when a non-smoke Tier 6 run is configured for less than one hour.
pub fn validate_soak_duration_for_profile(
    total: Duration,
    smoke_profile: bool,
) -> Result<(), String> {
    if !smoke_profile && total < MINIMUM_CANONICAL_SOAK_DURATION {
        return Err(
            "Tier 6 durations below 3600 seconds are only valid with STRESS_PROFILE=smoke"
                .to_string(),
        );
    }
    Ok(())
}

/// # Errors
///
/// Returns an error when measured samples is zero or the total duration cannot
/// provide at least one nanosecond per sample.
pub fn soak_sample_duration(total: Duration, measured_samples: usize) -> Result<Duration, String> {
    let samples = u32::try_from(measured_samples)
        .map_err(|_| "soak measured samples do not fit u32".to_string())?;
    if samples == 0 {
        return Err("soak measured samples must be positive".to_string());
    }
    let per_sample = total / samples;
    if per_sample.is_zero() {
        return Err("soak duration is too short for measured samples".to_string());
    }
    Ok(per_sample)
}
