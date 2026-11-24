//! Submit one native transaction, retaining existing unknown-outcome probes.
use super::WriteOptions;
#[cfg(any(test, feature = "storage-test-hooks"))]
use super::{INJECT_NATIVE_ABORT_UNKNOWN, INJECT_NATIVE_COMMIT_UNKNOWN};

pub(super) fn commit_native_transaction(
    transaction: cntryl_midge::Transaction,
    options: WriteOptions,
) -> Result<(), cntryl_midge::MidgeError> {
    #[cfg(any(test, feature = "storage-test-hooks"))]
    if INJECT_NATIVE_ABORT_UNKNOWN.with(|flag| flag.replace(false)) {
        drop(transaction);
        return Err(cntryl_midge::MidgeError::OutcomeUnknown(
            "injected unknown outcome after dropping uncommitted native transaction".to_owned(),
        ));
    }
    #[cfg(feature = "replay-benchmark-observation")]
    let result = super::super::native_metrics::observe_native_commit(options, || {
        transaction.commit(options)
    });
    #[cfg(not(feature = "replay-benchmark-observation"))]
    let result = transaction.commit(options);
    #[cfg(any(test, feature = "storage-test-hooks"))]
    if result.is_ok() && INJECT_NATIVE_COMMIT_UNKNOWN.with(|flag| flag.replace(false)) {
        return Err(cntryl_midge::MidgeError::OutcomeUnknown(
            "injected native commit response after actual successful commit".to_owned(),
        ));
    }
    result
}
