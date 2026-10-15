//! Bounds the test suite's temporary data directories.
//!
//! Every suite builds its fixture path under the system temp directory and
//! cleans it up with a `remove_dir_all` at the end of the test body. That line
//! is skipped whenever a test panics or returns early, so failing tests leaked
//! their data directories permanently — enough of them, over enough runs, to
//! fill the machine.
//!
//! [`sweep_stale_once`] removes what earlier runs left behind, once per test
//! process. It recognizes leftovers by the shared `cassie-` prefix, so a sweep
//! from any suite cleans up after every other suite too, and accumulation stays
//! bounded no matter which test panicked or how the run was interrupted.

#![allow(dead_code)]

use std::sync::Once;
use std::time::{Duration, SystemTime};

/// Directories older than this were left by a previous run: no live test holds
/// one this long, and the window is wide enough that a slow suite running in
/// parallel is never swept out from under itself.
const STALE_AFTER: Duration = Duration::from_hours(6);

/// Shared by every suite's data-directory helper.
const PREFIX: &str = "cassie-";

/// Removes data directories left behind by earlier runs, at most once per
/// process. Errors are ignored: another test process may be sweeping the same
/// entries concurrently, and losing that race is harmless.
pub fn sweep_stale_once() {
    static SWEPT: Once = Once::new();
    SWEPT.call_once(|| {
        let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
            return;
        };
        let now = SystemTime::now();
        for entry in entries.flatten() {
            if !entry.file_name().to_string_lossy().starts_with(PREFIX) {
                continue;
            }
            if is_stale(&entry, now) {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    });
}

fn is_stale(entry: &std::fs::DirEntry, now: SystemTime) -> bool {
    let Ok(metadata) = entry.metadata() else {
        return false;
    };
    if !metadata.is_dir() {
        return false;
    }
    let Ok(modified) = metadata.modified() else {
        return false;
    };
    now.duration_since(modified)
        .is_ok_and(|age| age >= STALE_AFTER)
}

#[cfg(test)]
mod tests {
    use super::{is_stale, sweep_stale_once, PREFIX, STALE_AFTER};

    use std::time::{Duration, SystemTime};

    use uuid::Uuid;

    #[test]
    fn should_keep_a_directory_a_running_test_still_owns() {
        // Arrange
        let path = std::env::temp_dir().join(format!("{PREFIX}sweep-fresh-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&path).expect("create directory");
        let entry = std::fs::read_dir(std::env::temp_dir())
            .expect("read temp dir")
            .flatten()
            .find(|entry| entry.path() == path)
            .expect("find the directory just created");

        // Act
        let stale = is_stale(&entry, SystemTime::now());

        // Assert
        assert!(!stale, "a directory created just now must not be swept");
        let _ = std::fs::remove_dir_all(&path);
    }

    #[test]
    fn should_treat_a_directory_older_than_the_window_as_stale() {
        // Arrange
        let path = std::env::temp_dir().join(format!("{PREFIX}sweep-old-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&path).expect("create directory");
        let entry = std::fs::read_dir(std::env::temp_dir())
            .expect("read temp dir")
            .flatten()
            .find(|entry| entry.path() == path)
            .expect("find the directory just created");
        // Ask the question as of a point far enough in the future that the
        // directory has aged past the window, rather than backdating the file.
        let later = SystemTime::now() + STALE_AFTER + Duration::from_secs(60);

        // Act
        let stale = is_stale(&entry, later);

        // Assert
        assert!(stale, "a directory older than the window must be swept");
        let _ = std::fs::remove_dir_all(&path);
    }

    #[test]
    fn should_sweep_only_once_per_process() {
        // Arrange
        sweep_stale_once();

        // Act
        sweep_stale_once();

        // Assert
        // Reaching here means repeated calls are safe; the `Once` makes the
        // second call a no-op rather than a second full directory scan.
    }
}
