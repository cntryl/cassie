//! Private completed-path diagnostics; test observations are confined to the calling thread.
#[cfg(test)]
std::thread_local! {
    static INPUT_HANDOFFS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static LAST_PATH: std::cell::Cell<Option<(&'static str, &'static str)>> = const { std::cell::Cell::new(None) };
}

pub(crate) fn publish(operator: &'static str, path: &'static str) {
    let state = if path.starts_with("scalar_") {
        "documented_scalar_boundary"
    } else {
        "accounted_blocking"
    };
    tracing::debug!(operator, path, state, "relational operator completed");
    #[cfg(test)]
    LAST_PATH.with(|last| last.set(Some((operator, path))));
}

#[cfg(test)]
pub(crate) fn last_path() -> Option<(&'static str, &'static str)> {
    LAST_PATH.with(std::cell::Cell::get)
}

#[cfg(test)]
pub(crate) fn input_handoff() {
    INPUT_HANDOFFS.with(|count| count.set(count.get() + 1));
}

#[cfg(test)]
pub(crate) fn input_handoffs() -> usize {
    INPUT_HANDOFFS.with(std::cell::Cell::get)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<String>>);
    impl tracing::Subscriber for Capture {
        fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn event(&self, event: &tracing::Event<'_>) {
            event.record(&mut self.clone());
        }
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
    }
    impl tracing::field::Visit for Capture {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            if field.name() == "state" {
                *self.0.lock().expect("state") = format!("{value:?}");
            }
        }
        fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
            if field.name() == "state" {
                *self.0.lock().expect("state") = value.into();
            }
        }
    }
    #[test]
    fn should_report_every_scalar_relational_boundary_without_bounded_promotion() {
        // Arrange
        let capture = Capture::default();
        for path in [
            "scalar_expression_order",
            "scalar_expression_window",
            "scalar_window_frame",
            "scalar_expression_distinct_on",
            "scalar_cte_materialization",
        ] {
            // Act
            tracing::subscriber::with_default(capture.clone(), || super::publish("probe", path));
            // Assert
            assert_eq!(
                *capture.0.lock().expect("state"),
                "documented_scalar_boundary",
                "{path}"
            );
        }
    }
}
