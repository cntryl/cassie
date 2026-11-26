//! Validity and target status recorded in the weighted route receipt.

use serde_json::Value;

pub(super) fn measurement_status(
    profile: &str,
    p95_ms: f64,
    reopen_valid: bool,
    outcomes: &[Value],
) -> (bool, Option<bool>) {
    let valid = reopen_valid
        && outcomes.len() == 100
        && outcomes
            .iter()
            .all(|row| row["matches_oracle"] == true && row["fallback"] == false);
    (
        valid,
        (profile == "full" && valid).then_some(p95_ms <= 100.0),
    )
}
