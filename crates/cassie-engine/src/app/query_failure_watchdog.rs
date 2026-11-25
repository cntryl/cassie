//! Owned child-process watchdog for failure-authority tests.

// Parent-owned process watchdog for potentially blocking retirement regressions.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

mod output;
use output::ChildOutput;

const CHILD_CASE: &str = "CASSIE_TEST_RETIREMENT_WATCHDOG_CASE";
const CHILD_TOKEN: &str = "CASSIE_TEST_RETIREMENT_WATCHDOG_TOKEN";
const CHILD_DIRECTORY: &str = "CASSIE_TEST_RETIREMENT_WATCHDOG_DIRECTORY";
const DEADLINE: Duration = Duration::from_secs(30);
const CHECK_INTERVAL: Duration = Duration::from_millis(20);
const RECEIPT_LIMIT: u64 = 256;

struct OwnedDirectory {
    path: PathBuf,
    removed: bool,
}

impl OwnedDirectory {
    fn new() -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!(
            "cassie-retirement-watchdog-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&path)
            .map_err(|error| format!("create watchdog directory: {error}"))?;
        Ok(Self {
            path,
            removed: false,
        })
    }

    fn remove(&mut self) -> Result<(), String> {
        std::fs::remove_dir_all(&self.path)
            .map_err(|error| format!("remove watchdog directory: {error}"))?;
        self.removed = true;
        Ok(())
    }
}

impl Drop for OwnedDirectory {
    fn drop(&mut self) {
        if !self.removed {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

struct OwnedChild {
    child: Child,
    reaped: bool,
    output: ChildOutput,
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.reaped {
            return;
        }
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn receipt(case: &str, token: &str, phase: &str) -> String {
    format!("CASSIE_RETIREMENT_WATCHDOG|{case}|{token}|{phase}\n")
}

fn child_environment(case: &str) -> (PathBuf, String) {
    let selected = std::env::var(CHILD_CASE).expect("watchdog child case");
    assert_eq!(selected, case, "selected watchdog child case");
    let token = std::env::var(CHILD_TOKEN).expect("watchdog child token");
    uuid::Uuid::parse_str(&token).expect("valid watchdog token");
    let directory = PathBuf::from(std::env::var_os(CHILD_DIRECTORY).expect("watchdog directory"));
    (directory, token)
}

fn write_receipt(directory: &Path, name: &str, content: &str) {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join(name))
        .expect("create unique watchdog receipt");
    file.write_all(content.as_bytes())
        .expect("write watchdog receipt");
    file.flush().expect("flush watchdog receipt");
}

fn verify_receipt(directory: &Path, name: &str, expected: &str) -> Result<(), String> {
    let file = std::fs::File::open(directory.join(name))
        .map_err(|error| format!("missing {name} receipt: {error}"))?;
    if file.metadata().map_err(|error| error.to_string())?.len() > RECEIPT_LIMIT {
        return Err(format!("oversized {name} receipt"));
    }
    let mut content = Vec::new();
    file.take(RECEIPT_LIMIT + 1)
        .read_to_end(&mut content)
        .map_err(|error| format!("read {name} receipt: {error}"))?;
    if content != expected.as_bytes() {
        return Err(format!("wrong {name} receipt"));
    }
    Ok(())
}

/// Return true only in the selected child; the parent waits for and verifies it.
#[doc(hidden)]
#[must_use]
pub fn enter_child(test_name: &str, case: &str) -> bool {
    if std::env::var_os(CHILD_CASE).is_some() {
        let (directory, token) = child_environment(case);
        write_receipt(&directory, "start", &receipt(case, &token, "START"));
        return true;
    }
    run_parent(test_name, case).expect("watchdog child completes and is reaped");
    false
}

#[doc(hidden)]
pub fn complete_child(case: &str) {
    let (directory, token) = child_environment(case);
    write_receipt(&directory, "complete", &receipt(case, &token, "COMPLETE"));
}

pub(super) fn run_parent(test_name: &str, case: &str) -> Result<(), String> {
    let mut directory = OwnedDirectory::new()?;
    let result = run_owned_child(&directory.path, test_name, case);
    let cleanup = directory.remove();
    match (result, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(error), Err(cleanup)) => Err(format!("{error}; {cleanup}")),
    }
}

fn run_owned_child(directory: &Path, test_name: &str, case: &str) -> Result<(), String> {
    let token = uuid::Uuid::new_v4().to_string();
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let mut child = Command::new(executable)
        .args(["--exact", test_name, "--nocapture", "--test-threads=1"])
        .env(CHILD_CASE, case)
        .env(CHILD_TOKEN, &token)
        .env(CHILD_DIRECTORY, directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("spawn watchdog child: {error}"))?;
    let output = ChildOutput::start(&mut child);
    let mut child = OwnedChild {
        child,
        reaped: false,
        output,
    };
    let deadline = Instant::now() + DEADLINE;
    let status = loop {
        if let Some(status) = child.child.try_wait().map_err(|error| error.to_string())? {
            child.reaped = true;
            break status;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            let _ = child.child.kill();
            child
                .child
                .wait()
                .map_err(|error| format!("reap timed-out child: {error}"))?;
            child.reaped = true;
            return Err(format!(
                "watchdog child timed out: {case}; {}",
                child.output.finish()
            ));
        }
        std::thread::park_timeout(CHECK_INTERVAL.min(remaining));
    };
    let output = child.output.finish();
    if !status.success() {
        return Err(format!(
            "watchdog child failed for {case}: {status}; {output}"
        ));
    }
    verify_receipt(directory, "start", &receipt(case, &token, "START"))
        .map_err(|error| format!("{error}; {output}"))?;
    verify_receipt(directory, "complete", &receipt(case, &token, "COMPLETE"))
        .map_err(|error| format!("{error}; {output}"))?;
    Ok(())
}

#[test]
fn should_reject_missing_retirement_watchdog_child() {
    // Arrange
    let name = "app::query_failure_watchdog::no_such_retirement_test";
    // Act
    let result = run_parent(name, "missing");
    // Assert
    let error = result.expect_err("missing watchdog child must fail");
    assert!(error.contains("missing start receipt"));
    assert!(error.contains("child stdout"));
    assert!(error.contains("0 tests"));
}
