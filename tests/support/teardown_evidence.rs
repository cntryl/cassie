//! Captures caller-thread teardown warnings without changing the global subscriber.

use std::io::{self, Write};
use std::sync::{Arc, Mutex};

struct Capture(Arc<Mutex<Vec<u8>>>);
struct CaptureWriter(Arc<Mutex<Vec<u8>>>);

impl Write for CaptureWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .expect("capture lock")
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'writer> tracing_subscriber::fmt::MakeWriter<'writer> for Capture {
    type Writer = CaptureWriter;

    fn make_writer(&'writer self) -> Self::Writer {
        CaptureWriter(Arc::clone(&self.0))
    }
}

/// Runs the owning fixture under a thread-scoped warning subscriber.
///
/// # Panics
///
/// Propagates fixture panics and rejects a poisoned capture or invalid UTF-8 output.
pub fn capture_warnings(run: impl FnOnce()) -> String {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_max_level(tracing::Level::WARN)
        .with_writer(Capture(Arc::clone(&bytes)))
        .finish();
    tracing::subscriber::with_default(subscriber, run);
    let captured = bytes.lock().expect("capture lock").clone();
    String::from_utf8(captured).expect("UTF8 tracing")
}
