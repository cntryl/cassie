//! Bounded synchronization helpers shared by unit and integration regressions.

use std::sync::{mpsc, Arc, Barrier};
use std::time::Duration;

const TEST_SYNC_DEADLINE: Duration = Duration::from_secs(120);

#[doc(hidden)]
pub fn wait_for_test_barrier(barrier: &Arc<Barrier>, label: &str) {
    let (sender, receiver) = mpsc::channel();
    let barrier = Arc::clone(barrier);
    std::thread::spawn(move || {
        barrier.wait();
        let _ = sender.send(());
    });
    receiver
        .recv_timeout(TEST_SYNC_DEADLINE)
        .unwrap_or_else(|error| panic!("{label} barrier deadline: {error}"));
}

#[doc(hidden)]
pub struct TestBarrierCleanup {
    barriers: Vec<Arc<Barrier>>,
    armed: bool,
}

impl TestBarrierCleanup {
    #[doc(hidden)]
    #[must_use]
    pub fn arm(barriers: impl IntoIterator<Item = Arc<Barrier>>) -> Self {
        Self {
            barriers: barriers.into_iter().collect(),
            armed: true,
        }
    }

    #[doc(hidden)]
    pub fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TestBarrierCleanup {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        for barrier in &self.barriers {
            let barrier = Arc::clone(barrier);
            std::thread::spawn(move || {
                barrier.wait();
            });
        }
    }
}

#[doc(hidden)]
#[must_use]
pub fn join_test_thread<T: Send + 'static>(handle: std::thread::JoinHandle<T>, label: &str) -> T {
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let result = handle.join();
        let _ = sender.send(result);
    });
    receiver
        .recv_timeout(TEST_SYNC_DEADLINE)
        .unwrap_or_else(|error| panic!("{label} completion deadline: {error}"))
        .unwrap_or_else(|_| panic!("{label} thread panicked"))
}

#[doc(hidden)]
#[must_use]
pub fn recv_test<T>(receiver: &mpsc::Receiver<T>, label: &str) -> T {
    receiver
        .recv_timeout(TEST_SYNC_DEADLINE)
        .unwrap_or_else(|error| panic!("{label} receive deadline: {error}"))
}
