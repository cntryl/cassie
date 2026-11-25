use std::time::{Duration, Instant};

use crate::stress::ExternalSample;

pub fn sample_until_deadline<F>(sample_duration: Duration, mut operation: F) -> ExternalSample
where
    F: FnMut() -> u64,
{
    let started = Instant::now();
    let mut completed_operations = 0_u64;
    loop {
        let completed = operation();
        assert!(completed > 0, "transport operation must make progress");
        completed_operations = completed_operations
            .checked_add(completed)
            .expect("transport operation count should fit u64");
        if started.elapsed() >= sample_duration {
            break;
        }
    }
    ExternalSample::new(started.elapsed(), completed_operations)
}
