//! Test-only adapter hook that reports the actual Data commit result to Engine.
use std::sync::OnceLock;

/// One actual native Data submission observed at the adapter commit boundary.
#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct DataSubmission {
    pub policy: &'static str,
    pub succeeded: bool,
    pub outcome_unknown: bool,
}

/// Engine-side sink for real native Data submission outcomes.
#[doc(hidden)]
pub type DataSubmissionObserver = fn(DataSubmission);
static OBSERVER: OnceLock<DataSubmissionObserver> = OnceLock::new();

/// Installs the Engine-side recorder used by feature-gated retry-policy tests.
#[doc(hidden)]
pub fn install_data_submission_observer(observer: DataSubmissionObserver) {
    let _ = OBSERVER.set(observer);
}

/// Target the existing adapter-response simulation at Data, not an earlier
/// schema epoch commit. The native result is not replaced by this observer.
#[doc(hidden)]
pub fn arm_existing_unknown_on_next_data_submission() {
    DATA_POLICY_UNKNOWN_ON_DATA.with(|flag| flag.set(true));
}

/// Clears a pending response simulation when the observing operation exits.
#[doc(hidden)]
pub fn clear_existing_unknown_on_next_data_submission() {
    DATA_POLICY_UNKNOWN_ON_DATA.with(|flag| flag.set(false));
}

thread_local! {
    static DATA_POLICY_UNKNOWN_ON_DATA: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub(super) fn observe_data_submission<T>(
    options: cntryl_midge::WriteOptions,
    submit: impl FnOnce() -> Result<T, cntryl_midge::MidgeError>,
) -> Result<T, cntryl_midge::MidgeError> {
    // Invoked inside the owning row-publication callback, after all staging /
    // suppression branches and publication permission. Record the real result.
    if DATA_POLICY_UNKNOWN_ON_DATA.with(|flag| flag.replace(false)) {
        super::inject_native_commit_unknown_after_success();
    }
    let result = submit();
    let policy = match options.policy() {
        cntryl_midge::DurabilityPolicy::Sync => "local_sync",
        cntryl_midge::DurabilityPolicy::Buffered => "local_buffered",
        cntryl_midge::DurabilityPolicy::CloudAsync => "cloud_async",
        cntryl_midge::DurabilityPolicy::CloudStrict => "cloud_strict",
        cntryl_midge::DurabilityPolicy::BestEffort => "best_effort",
    };
    if let Some(observer) = OBSERVER.get() {
        observer(DataSubmission {
            policy,
            succeeded: result.is_ok(),
            outcome_unknown: matches!(
                &result,
                Err(cntryl_midge::MidgeError::OutcomeUnknown(_))
            ),
        });
    }
    result
}
