//! Cleanup guard for on-disk benchmark fixtures.
//!
//! Benchmarks used to remove their data directory with a trailing
//! `remove_dir_all(...).expect(...)` after `runner.finish()`. That line is
//! skipped whenever a measurement assertion or an evidence check panics —
//! exactly the runs a benchmark gate exists to produce — so a failing nightly
//! left a full-scale fixture on disk every time. It also turned a cleanup
//! failure into a benchmark failure after the measurement had already
//! succeeded.
//!
//! [`FixtureDir`] removes the directory from `Drop` instead, which runs while
//! a panicking benchmark unwinds, and ignores a cleanup error rather than
//! masking the panic that caused it.

use std::path::PathBuf;

pub struct FixtureDir {
    path: PathBuf,
}

impl FixtureDir {
    /// Takes ownership of `path` for cleanup. Declare this immediately after
    /// building the context so the guard covers every later panic site.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

impl Drop for FixtureDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
