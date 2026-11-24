//! Attribution of caller-thread native commits to a post-write path.
//! Always compiled; a no-op without the replay benchmark observation feature.

#[cfg(feature = "replay-benchmark-observation")]
use std::cell::Cell;

/// The post-write path a caller-thread native commit belongs to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum CommitSource {
    #[default]
    Primary,
    ColumnMaintenance,
    HashMaintenance,
    TimeSeriesMaintenance,
}

#[cfg(feature = "replay-benchmark-observation")]
thread_local! {
    static SOURCE: Cell<CommitSource> = const { Cell::new(CommitSource::Primary) };
}

#[cfg(feature = "replay-benchmark-observation")]
pub(super) fn current_commit_source() -> CommitSource {
    SOURCE.get()
}

/// Attributes native commits on this thread to a maintenance path while held.
/// The outermost maintenance scope wins, so nested helpers keep their caller's
/// attribution. Without the observation feature this is a no-op.
pub(super) struct CommitSourceScope {
    #[cfg(feature = "replay-benchmark-observation")]
    previous: Option<CommitSource>,
}

impl CommitSourceScope {
    #[cfg(feature = "replay-benchmark-observation")]
    pub(super) fn enter(source: CommitSource) -> Self {
        let current = SOURCE.get();
        if current == CommitSource::Primary {
            SOURCE.set(source);
            Self {
                previous: Some(current),
            }
        } else {
            Self { previous: None }
        }
    }

    #[cfg(not(feature = "replay-benchmark-observation"))]
    pub(super) const fn enter(_source: CommitSource) -> Self {
        Self {}
    }
}

#[cfg(feature = "replay-benchmark-observation")]
impl Drop for CommitSourceScope {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            SOURCE.set(previous);
        }
    }
}
