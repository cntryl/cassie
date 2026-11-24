//! Isolates process-wide authentication tracing without exposing captured secrets.

use std::io::{Read, Write};
use std::path::PathBuf;

#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const PROBE_DIRECTORY_ENV: &str = "CASSIE_AUTH03_OWNED_DIRECTORY";
const LOG_LIMIT: usize = 1_048_576;
const OUTPUT_LIMIT: usize = 65_536;
const CHILD_LIMIT: Duration = Duration::from_secs(90);
const ASYNC_SENTINEL: &str = "auth03 benign async capture sentinel";
const BLOCKING_SENTINEL: &str = "auth03 benign blocking capture sentinel";

#[derive(Clone, Default)]
pub struct TraceCapture(Arc<Mutex<Captured>>);

#[derive(Default)]
struct Captured {
    bytes: Vec<u8>,
    overflow: bool,
}

pub struct TraceWriter(TraceCapture);

impl Write for TraceWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let mut captured = self.0 .0.lock().expect("trace buffer lock");
        let available = LOG_LIMIT.saturating_sub(captured.bytes.len());
        captured
            .bytes
            .extend_from_slice(&bytes[..bytes.len().min(available)]);
        captured.overflow |= bytes.len() > available;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'writer> tracing_subscriber::fmt::MakeWriter<'writer> for TraceCapture {
    type Writer = TraceWriter;

    fn make_writer(&'writer self) -> Self::Writer {
        TraceWriter(self.clone())
    }
}

impl TraceCapture {
    pub fn install_in_owned_child() -> Self {
        let capture = Self::default();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .without_time()
            .with_max_level(tracing::Level::TRACE)
            .with_writer(capture.clone())
            .finish();
        assert!(
            tracing::subscriber::set_global_default(subscriber).is_ok(),
            "owned child must install its own global subscriber"
        );
        capture
    }

    pub fn assert_redacted_with_both_domains(&self, canaries: &[String]) {
        let captured = self.0.lock().expect("trace buffer lock");
        assert!(
            !captured.overflow,
            "trace capture must be complete within its bound"
        );
        assert_secret_free(&captured.bytes, canaries);
        let text = String::from_utf8_lossy(&captured.bytes);
        assert!(
            text.contains(ASYNC_SENTINEL),
            "async trace capture positive control"
        );
        assert!(
            text.contains(BLOCKING_SENTINEL),
            "blocking trace capture positive control"
        );
    }
}

pub async fn emit_capture_positive_controls() {
    tokio::spawn(async {
        tracing::info!("auth03 benign async capture sentinel");
    })
    .await
    .expect("async sentinel task");
    tokio::task::spawn_blocking(|| {
        tracing::info!("auth03 benign blocking capture sentinel");
    })
    .await
    .expect("blocking sentinel task");
}

pub fn assert_secret_free(bytes: &[u8], canaries: &[String]) {
    assert!(
        canaries.iter().all(|canary| !canary.is_empty()),
        "nonempty secret canaries"
    );
    assert!(
        canaries.iter().all(|canary| !bytes
            .windows(canary.len())
            .any(|window| window == canary.as_bytes())),
        "credential canary leaked"
    );
}

pub fn owned_storage_path() -> PathBuf {
    PathBuf::from(std::env::var_os(PROBE_DIRECTORY_ENV).expect("owned probe directory"))
        .join("store")
}

pub fn retain_private_canaries(canaries: &[String]) {
    let path = owned_storage_path().with_file_name("canaries.json");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(path).expect("private canary receipt");
    let bytes = serde_json::to_vec(canaries).expect("private canary encoding");
    file.write_all(&bytes)
        .expect("private canary receipt write");
    file.sync_all().expect("private canary receipt sync");
}

struct OwnedProbeDirectory(PathBuf);

impl OwnedProbeDirectory {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("cassie-auth03-capture-{}", uuid::Uuid::new_v4()));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        builder.mode(0o700);
        builder
            .create(&path)
            .expect("private owned probe directory");
        Self(path)
    }

    fn read_canaries(&self) -> Option<Vec<String>> {
        let path = self.0.join("canaries.json");
        let metadata = std::fs::metadata(&path).ok()?;
        if metadata.len() > u64::try_from(OUTPUT_LIMIT).ok()? {
            return None;
        }
        let bytes = std::fs::read(path).ok()?;
        let canaries: Vec<String> = serde_json::from_slice(&bytes).ok()?;
        (canaries.len() == 6
            && canaries
                .iter()
                .all(|value| !value.is_empty() && value.len() <= 4096))
        .then_some(canaries)
    }

    fn cleanup(&self) {
        std::fs::remove_dir_all(&self.0).expect("cleanup owned child store and private receipt");
        assert!(!self.0.exists(), "owned probe directory removed");
    }
}

impl Drop for OwnedProbeDirectory {
    fn drop(&mut self) {
        if self.0.exists() {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

struct OwnedChild(Child);

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn drain_bounded(mut stream: impl Read) -> (Vec<u8>, bool) {
    let mut captured = Vec::new();
    let mut complete = true;
    let mut buffer = [0_u8; 4096];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(length) => {
                let available = OUTPUT_LIMIT.saturating_sub(captured.len());
                captured.extend_from_slice(&buffer[..length.min(available)]);
                complete &= length <= available;
            }
            Err(_) => {
                complete = false;
                break;
            }
        }
    }
    (captured, complete)
}

/// Returns true only in the selected child; the parent owns its bounded lifetime.
pub fn enter_owned_child(test_name: &str, flag: &str, completion: &str) -> bool {
    if std::env::var_os(flag).is_some() {
        return true;
    }
    let directory = OwnedProbeDirectory::new();
    let mut child = OwnedChild(
        Command::new(std::env::current_exe().expect("test executable"))
            .args(["--exact", test_name, "--nocapture", "--test-threads=1"])
            .env(flag, "1")
            .env(PROBE_DIRECTORY_ENV, &directory.0)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("owned authentication capture child"),
    );
    let stdout = child.0.stdout.take().expect("child stdout pipe");
    let stderr = child.0.stderr.take().expect("child stderr pipe");
    let stdout = std::thread::spawn(move || drain_bounded(stdout));
    let stderr = std::thread::spawn(move || drain_bounded(stderr));
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.0.try_wait().expect("child status") {
            break Some(status);
        }
        if started.elapsed() >= CHILD_LIMIT {
            child.0.kill().expect("stop timed-out owned child");
            child.0.wait().expect("reap timed-out owned child");
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    drop(child);
    let (stdout, stdout_complete) = stdout.join().expect("stdout drain");
    let (stderr, stderr_complete) = stderr.join().expect("stderr drain");
    let canaries = directory.read_canaries();
    directory.cleanup();
    // Never print captured output: a failing regression may contain a credential.
    assert!(
        stdout_complete && stderr_complete,
        "bounded child output must be complete"
    );
    let canaries = canaries.expect("complete private canary receipt required");
    assert_secret_free(&stdout, &canaries);
    assert_secret_free(&stderr, &canaries);
    assert!(
        status.is_some_and(|status| status.success()),
        "owned authentication child failed"
    );
    assert!(
        String::from_utf8_lossy(&stdout)
            .lines()
            .any(|line| line == completion),
        "selected child must execute all assertions, not zero tests"
    );
    false
}
