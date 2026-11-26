use std::time::Duration;

use super::Cassie;

impl Cassie {
    pub(crate) fn with_projection_publication<T, E>(
        &self,
        projection: &str,
        check: impl Fn() -> Result<(), E>,
        publish: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E> {
        check()?;
        let gate = self.midge.projection_publication_gate(projection);
        let _guard = loop {
            if let Some(guard) = gate.try_lock_for(Duration::from_millis(2)) {
                break guard;
            }
            crate::executor::observe_projection_publication_wait();
            check()?;
        };
        check()?;
        publish()
    }
}
