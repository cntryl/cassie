//! Write-stall retries for bulk vector index writes.
//!
//! Midge refuses a transaction with `WriteStall` at admission, before anything is applied, while
//! its memtables wait for a flush. A vector index build commits many byte-bounded transactions
//! back to back, so at large row counts and dimensions it outruns flushing and meets that
//! backpressure. A single document write gets a short retry budget; a bulk build waits longer,
//! because the stall clears only once the flush it is waiting on completes.

use std::time::Duration;

use crate::app::{CassieError, StorageRetryKind};

const BULK_WRITE_STALL_ATTEMPTS: u32 = 400;
const BULK_WRITE_STALL_STEP: Duration = Duration::from_millis(5);
const BULK_WRITE_STALL_MAX_DELAY: Duration = Duration::from_millis(250);

/// How long to wait before retrying a bulk index write, or `None` when the error is returned.
/// Only write stalls retry: they were refused before apply, so the retried transaction is the
/// first one to take effect.
pub(incrate::adapter) fn bulk_write_stall_delay(
    error: &CassieError,
    attempts: u32,
) -> Option<Duration> {
    if attempts >= BULK_WRITE_STALL_ATTEMPTS
        || error.storage_retry_kind() != Some(StorageRetryKind::WriteStall)
    {
        return None;
    }
    Some(
        BULK_WRITE_STALL_STEP
            .saturating_mul(attempts)
            .min(BULK_WRITE_STALL_MAX_DELAY),
    )
}

/// Runs `write`, which must build and commit one whole transaction, until it succeeds or fails
/// with an error [`bulk_write_stall_delay`] does not retry.
pub(incrate::adapter) fn retry_bulk_write_stall(
    mut write: impl FnMut() -> Result<(), CassieError>,
) -> Result<(), CassieError> {
    let mut attempts = 1_u32;
    loop {
        match write() {
            Ok(()) => return Ok(()),
            Err(error) => {
                let Some(delay) = bulk_write_stall_delay(&error, attempts) else {
                    return Err(error);
                };
                attempts = attempts.saturating_add(1);
                std::thread::sleep(delay);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{bulk_write_stall_delay, retry_bulk_write_stall, BULK_WRITE_STALL_ATTEMPTS};
    use crate::adapter::CassieError;
    use std::time::Duration;

    fn stall() -> CassieError {
        CassieError::from(cntryl_midge::MidgeError::WriteStall(
            "memtable full".to_string(),
        ))
    }

    #[test]
    fn should_back_off_a_bulk_write_stall_up_to_a_capped_delay() {
        // Arrange
        let error = stall();

        // Act
        let delays = [1, 2, 100].map(|attempts| bulk_write_stall_delay(&error, attempts));

        // Assert
        assert_eq!(
            delays,
            [
                Some(Duration::from_millis(5)),
                Some(Duration::from_millis(10)),
                Some(Duration::from_millis(250)),
            ]
        );
    }

    #[test]
    fn should_return_a_bulk_write_stall_once_the_attempts_are_spent() {
        // Arrange
        let error = stall();

        // Act
        let delay = bulk_write_stall_delay(&error, BULK_WRITE_STALL_ATTEMPTS);

        // Assert
        assert_eq!(delay, None);
    }

    #[test]
    fn should_not_retry_a_bulk_write_conflict() {
        // Arrange
        let error = CassieError::from(cntryl_midge::MidgeError::WriteConflict(
            "overlap".to_string(),
        ));

        // Act
        let delay = bulk_write_stall_delay(&error, 1);

        // Assert
        assert_eq!(delay, None);
    }

    #[test]
    fn should_rerun_the_whole_transaction_after_each_write_stall() {
        // Arrange
        let mut calls = 0_u32;

        // Act
        let result = retry_bulk_write_stall(|| {
            calls += 1;
            if calls < 4 {
                Err(stall())
            } else {
                Ok(())
            }
        });

        // Assert
        assert!(result.is_ok(), "the stall was not retried");
        assert_eq!(calls, 4);
    }
}
