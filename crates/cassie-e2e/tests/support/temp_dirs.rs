//! Allocates data paths inside a process-owned test fixture directory.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};
use uuid::Uuid;

const FIXTURE_NAMESPACE: &str = "cassie-e2e-fixtures-v1";
const OWNER_MARKER: &str = ".cassie-e2e-owner";
const RUN_PREFIX: &str = "cassie-run-";
const STALE_AFTER: Duration = Duration::from_hours(6);

/// An isolated test-owned directory removed after its storage handles close.
pub struct IsolatedDataDir(PathBuf);

impl IsolatedDataDir {
    /// # Panics
    ///
    /// Panics if the fixture namespace or ownership marker cannot be created.
    #[must_use]
    pub fn new(label: &str) -> Self {
        Self(data_path(label))
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for IsolatedDataDir {
    fn drop(&mut self) {
        if !std::thread::panicking() && self.0.exists() {
            fs::remove_dir_all(&self.0).expect("remove isolated test data directory");
        }
    }
}

/// Returns a fresh path without creating a Midge store at that path.
///
/// # Panics
///
/// Panics if the fixture namespace or ownership marker cannot be created.
#[must_use]
pub fn data_path(label: &str) -> PathBuf {
    static RUN: OnceLock<FixtureRun> = OnceLock::new();
    let run = RUN.get_or_init(|| {
        let namespace = std::env::temp_dir().join(FIXTURE_NAMESPACE);
        fs::create_dir_all(&namespace).expect("create e2e fixture namespace");
        assert!(
            fs::symlink_metadata(&namespace).is_ok_and(|entry| entry.is_dir()),
            "e2e fixture namespace must be a directory"
        );
        sweep_stale(&namespace, SystemTime::now());
        FixtureRun::create(&namespace).expect("create owned e2e fixture run")
    });
    run.path.join(format!("cassie-{label}-{}", Uuid::new_v4()))
}

struct FixtureRun {
    path: PathBuf,
    _owner: File,
}

impl FixtureRun {
    fn create(namespace: &Path) -> io::Result<Self> {
        let id = Uuid::new_v4().to_string();
        let path = namespace.join(format!("{RUN_PREFIX}{id}"));
        fs::create_dir(&path)?;

        let mut owner = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path.join(OWNER_MARKER))?;
        owner.try_lock()?;
        owner.write_all(id.as_bytes())?;
        Ok(Self {
            path,
            _owner: owner,
        })
    }
}

fn sweep_stale(namespace: &Path, now: SystemTime) {
    let Ok(entries) = fs::read_dir(namespace) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(id) = name.to_str().and_then(|name| name.strip_prefix(RUN_PREFIX)) else {
            continue;
        };
        if Uuid::parse_str(id).is_err() || !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let path = entry.path();
        if let Some(_owner) = lock_stale_owner(&path, id, now) {
            // Hold the sweep lock until after the filesystem operation, including
            // when another test process is sweeping the same namespace.
            let _ = fs::remove_dir_all(path);
        }
    }
}

fn lock_stale_owner(path: &Path, id: &str, now: SystemTime) -> Option<File> {
    let marker = path.join(OWNER_MARKER);
    let metadata = fs::symlink_metadata(&marker).ok()?;
    if !metadata.is_file()
        || !now
            .duration_since(metadata.modified().ok()?)
            .is_ok_and(|age| age >= STALE_AFTER)
    {
        return None;
    }
    let mut owner = OpenOptions::new()
        .read(true)
        .write(true)
        .open(marker)
        .ok()?;
    owner.try_lock().ok()?;
    let mut recorded = String::new();
    // Canonical UUID strings occupy 36 bytes. Read one extra byte to reject
    // markers with trailing content.
    Read::by_ref(&mut owner)
        .take(37)
        .read_to_string(&mut recorded)
        .ok()?;
    (recorded == id).then_some(owner)
}

#[cfg(test)]
mod tests {
    use super::{sweep_stale, FixtureRun, IsolatedDataDir, OWNER_MARKER, RUN_PREFIX, STALE_AFTER};
    use std::fs::{self, FileTimes};
    use std::time::{Duration, SystemTime};

    #[test]
    fn should_sweep_only_stale_owned_runs_without_a_live_lock() {
        // Arrange
        let namespace = IsolatedDataDir::new("sweep-namespace");
        fs::create_dir(namespace.path()).expect("create sweep namespace");
        let stale = FixtureRun::create(namespace.path()).expect("stale run");
        let fresh = FixtureRun::create(namespace.path()).expect("fresh run");
        let active = FixtureRun::create(namespace.path()).expect("active run");
        let unowned = namespace
            .path()
            .join(format!("{RUN_PREFIX}{}", uuid::Uuid::new_v4()));
        fs::create_dir(&unowned).expect("unowned directory");
        fs::write(unowned.join("data"), b"keep user data").expect("unowned data");
        let now = SystemTime::now();
        let old = now - STALE_AFTER - Duration::from_mins(1);
        let old_times = FileTimes::new().set_modified(old);
        for run in [&stale, &active] {
            fs::OpenOptions::new()
                .write(true)
                .open(run.path.join(OWNER_MARKER))
                .expect("open owner marker")
                .set_times(old_times)
                .expect("age owner marker");
        }
        fs::File::open(&unowned)
            .expect("open unowned directory")
            .set_times(old_times)
            .expect("age unowned directory");
        let stale_path = stale.path.clone();
        let fresh_path = fresh.path.clone();
        drop(stale);
        drop(fresh);

        // Act
        sweep_stale(namespace.path(), now);

        // Assert
        assert!(!stale_path.exists(), "collect the old abandoned run");
        assert!(fresh_path.is_dir(), "keep the recent abandoned run");
        assert!(
            active.path.is_dir(),
            "keep the old run that still holds its lock"
        );
        assert_eq!(
            fs::read(unowned.join("data")).expect("unowned data"),
            b"keep user data"
        );
    }
}
