//! Drain private child pipes without retaining unbounded diagnostic output.
use std::io::{self, Read};
use std::process::Child;
use std::thread::JoinHandle;

const OUTPUT_LIMIT: usize = 16 * 1024;

struct Captured {
    tail: Vec<u8>,
    total: usize,
}

type Reader = JoinHandle<io::Result<Captured>>;

pub(super) struct ChildOutput {
    stdout: Option<Reader>,
    stderr: Option<Reader>,
}

fn capture(mut reader: impl Read, limit: usize) -> io::Result<Captured> {
    let mut tail = Vec::with_capacity(limit);
    let mut total = 0_usize;
    let mut chunk = [0_u8; 4096];
    loop {
        let read = match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        total = total.saturating_add(read);
        if read >= limit {
            tail.clear();
            tail.extend_from_slice(&chunk[read - limit..read]);
        } else {
            let overflow = tail.len().saturating_add(read).saturating_sub(limit);
            tail.drain(..overflow);
            tail.extend_from_slice(&chunk[..read]);
        }
    }
    Ok(Captured { tail, total })
}

fn collect(reader: Option<Reader>, label: &str) -> String {
    match reader.map(JoinHandle::join) {
        Some(Ok(Ok(output))) => format!(
            "{label} (retained tail {} of {} bytes):\n{}",
            output.tail.len(),
            output.total,
            String::from_utf8_lossy(&output.tail)
        ),
        Some(Ok(Err(error))) => format!("{label} capture failed: {error}"),
        Some(Err(_)) => format!("{label} capture reader panicked"),
        None => String::new(),
    }
}

impl ChildOutput {
    pub(super) fn start(child: &mut Child) -> Self {
        let stdout = child.stdout.take().expect("private child stdout pipe");
        let stderr = child.stderr.take().expect("private child stderr pipe");
        Self {
            stdout: Some(std::thread::spawn(move || capture(stdout, OUTPUT_LIMIT))),
            stderr: Some(std::thread::spawn(move || capture(stderr, OUTPUT_LIMIT))),
        }
    }

    /// Caller has reaped the child, so both pipe readers can finish at EOF.
    pub(super) fn finish(&mut self) -> String {
        format!(
            "{}\n{}",
            collect(self.stdout.take(), "child stdout"),
            collect(self.stderr.take(), "child stderr")
        )
    }
}

impl Drop for ChildOutput {
    fn drop(&mut self) {
        // OwnedChild kills/reaps before its fields are dropped.
        let _ = collect(self.stdout.take(), "stdout");
        let _ = collect(self.stderr.take(), "stderr");
    }
}

#[test]
fn should_drain_child_diagnostics_with_a_bounded_retained_tail() {
    // Arrange
    let bytes = b"long prefix panic details";

    // Act
    let output = capture(std::io::Cursor::new(bytes), 13).unwrap();
    let short = capture(std::io::Cursor::new(b"short"), 13).unwrap();

    // Assert
    assert_eq!(output.tail, b"panic details");
    assert_eq!(output.total, bytes.len());
    assert_eq!(short.tail, b"short");
    assert_eq!(short.total, 5);
}
