pub mod accounted;
pub mod text;

#[cfg(test)]
mod tests;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// Shared cancellation state for one request.
#[derive(Debug, Clone)]
pub struct RequestCancellation {
    canceled: Arc<AtomicBool>,
    signal: tokio::sync::watch::Sender<bool>,
}

impl Default for RequestCancellation {
    fn default() -> Self {
        let (signal, _) = tokio::sync::watch::channel(false);
        Self {
            canceled: Arc::new(AtomicBool::new(false)),
            signal,
        }
    }
}

impl RequestCancellation {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.canceled.store(true, Ordering::Release);
        self.signal.send_replace(true);
    }

    #[must_use]
    pub fn is_canceled(&self) -> bool {
        self.canceled.load(Ordering::Acquire)
    }

    /// Subscribes to the live request signal, including cancellation that happened earlier.
    #[must_use]
    pub fn subscribe(&self) -> tokio::sync::watch::Receiver<bool> {
        self.signal.subscribe()
    }

    pub async fn wait_canceled(&self) {
        let mut signal = self.subscribe();
        while !*signal.borrow_and_update() {
            if signal.changed().await.is_err() {
                return;
            }
        }
    }

    #[must_use]
    pub fn is_same_request(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.canceled, &other.canceled)
    }

    #[must_use]
    pub fn downgrade(&self) -> WeakRequestCancellation {
        WeakRequestCancellation(Arc::downgrade(&self.canceled))
    }
}

/// Non-retaining request identity probe for lifecycle observations.
#[derive(Debug, Clone)]
pub struct WeakRequestCancellation(std::sync::Weak<AtomicBool>);

impl WeakRequestCancellation {
    #[must_use]
    pub fn is_alive(&self) -> bool {
        self.0.strong_count() > 0
    }
}

/// Typed interruption result shared by query engines and retrieval providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptionError {
    Canceled,
    DeadlineExceeded,
}

/// A live cancellation borrow and an absolute request deadline.
#[derive(Debug, Clone, Copy)]
pub struct Interruption<'a> {
    deadline: Option<Instant>,
    cancellation: &'a RequestCancellation,
}

impl<'a> Interruption<'a> {
    #[must_use]
    pub const fn new(deadline: Option<Instant>, cancellation: &'a RequestCancellation) -> Self {
        Self {
            deadline,
            cancellation,
        }
    }

    #[must_use]
    pub const fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    #[must_use]
    pub const fn cancellation(&self) -> &'a RequestCancellation {
        self.cancellation
    }

    pub fn check(&self) -> Result<(), InterruptionError> {
        if self.cancellation.is_canceled() {
            return Err(InterruptionError::Canceled);
        }
        if self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Err(InterruptionError::DeadlineExceeded);
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct RequestMemoryTracker {
    budget: usize,
    used: AtomicUsize,
    peak: AtomicUsize,
}

impl RequestMemoryTracker {
    #[must_use]
    pub fn current_bytes(&self) -> usize {
        self.used.load(Ordering::Acquire)
    }

    #[must_use]
    pub fn peak_bytes(&self) -> usize {
        self.peak.load(Ordering::Acquire).max(self.current_bytes())
    }
}

/// Shared atomic accounting for one request's retained-memory leases.
#[derive(Debug, Clone)]
pub struct RequestMemory(Arc<RequestMemoryTracker>);

impl RequestMemory {
    #[must_use]
    pub fn new(budget: usize) -> Self {
        Self(Arc::new(RequestMemoryTracker {
            budget,
            used: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
        }))
    }

    /// Creates a standalone lease, used when a bounded owner outlives its request controls.
    pub fn reserve(&self, bytes: usize) -> Result<RequestMemoryReservation, MemoryBudgetError> {
        reserve_tracker_memory(&self.0, bytes)?;
        Ok(RequestMemoryReservation {
            tracker: Arc::clone(&self.0),
            bytes,
        })
    }

    #[must_use]
    pub fn current_bytes(&self) -> usize {
        self.0.used.load(Ordering::Acquire)
    }

    #[must_use]
    pub fn peak_bytes(&self) -> usize {
        self.0.peak.load(Ordering::Acquire)
    }

    #[must_use]
    pub fn budget(&self) -> usize {
        self.0.budget
    }

    #[must_use]
    pub fn downgrade(&self) -> std::sync::Weak<RequestMemoryTracker> {
        Arc::downgrade(&self.0)
    }
}

/// Memory budget admission failure with the exact configured and requested sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryBudgetError {
    pub budget: usize,
    pub requested: usize,
}

/// One RAII lease against a request's atomic memory tracker.
#[derive(Debug)]
pub struct RequestMemoryReservation {
    tracker: Arc<RequestMemoryTracker>,
    bytes: usize,
}

impl RequestMemoryReservation {
    #[must_use]
    pub const fn bytes(&self) -> usize {
        self.bytes
    }

    #[must_use]
    pub fn belongs_to(&self, memory: &RequestMemory) -> bool {
        Arc::ptr_eq(&self.tracker, &memory.0)
    }

    #[must_use]
    pub fn same_tracker(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.tracker, &other.tracker)
    }

    #[must_use]
    pub fn tracker_peak_bytes(&self) -> usize {
        self.tracker.peak.load(Ordering::Acquire)
    }

    #[must_use]
    pub fn tracker_used_bytes(&self) -> usize {
        self.tracker.used.load(Ordering::Acquire)
    }

    pub fn try_sibling(&self, bytes: usize) -> Result<Self, MemoryBudgetError> {
        reserve_tracker_memory(&self.tracker, bytes)?;
        Ok(Self {
            tracker: Arc::clone(&self.tracker),
            bytes,
        })
    }

    /// Transfers a same-tracker lease without changing global accounting.
    pub fn absorb(&mut self, mut other: Self) {
        assert!(Arc::ptr_eq(&self.tracker, &other.tracker));
        self.bytes = self
            .bytes
            .checked_add(other.bytes)
            .expect("admitted memory sum");
        other.bytes = 0;
    }

    pub fn try_grow(&mut self, additional_bytes: usize) -> Result<(), MemoryBudgetError> {
        reserve_tracker_memory(&self.tracker, additional_bytes)?;
        self.bytes = self
            .bytes
            .checked_add(additional_bytes)
            .expect("reservation growth is bounded by the shared tracker");
        Ok(())
    }

    pub fn shrink_to(&mut self, retained_bytes: usize) {
        assert!(
            retained_bytes <= self.bytes,
            "cannot grow a request memory reservation by shrinking it"
        );
        let released = self.bytes - retained_bytes;
        self.bytes = retained_bytes;
        self.tracker.used.fetch_sub(released, Ordering::AcqRel);
    }
}

impl Drop for RequestMemoryReservation {
    fn drop(&mut self) {
        self.tracker.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

fn reserve_tracker_memory(
    tracker: &RequestMemoryTracker,
    bytes: usize,
) -> Result<(), MemoryBudgetError> {
    let mut used = tracker.used.load(Ordering::Acquire);
    loop {
        let Some(next) = used.checked_add(bytes) else {
            return Err(MemoryBudgetError {
                budget: tracker.budget,
                requested: usize::MAX,
            });
        };
        if next > tracker.budget {
            return Err(MemoryBudgetError {
                budget: tracker.budget,
                requested: next,
            });
        }
        match tracker
            .used
            .compare_exchange_weak(used, next, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => {
                tracker.peak.fetch_max(next, Ordering::AcqRel);
                return Ok(());
            }
            Err(actual) => used = actual,
        }
    }
}
