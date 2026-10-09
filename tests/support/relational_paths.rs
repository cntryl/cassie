//! Caller-thread completed-operator events; never a global last-path observation.
use std::sync::{Arc, Mutex};

#[derive(Debug, PartialEq, Eq)]
pub struct Event {
    pub operator: String,
    pub path: String,
    pub state: String,
}
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<Event>>>);
#[derive(Default)]
struct Fields {
    operator: Option<String>,
    path: Option<String>,
    state: Option<String>,
}
impl tracing::field::Visit for Fields {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.record_str(field, &format!("{value:?}"));
    }
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        match field.name() {
            "operator" => self.operator = Some(value.into()),
            "path" => self.path = Some(value.into()),
            "state" => self.state = Some(value.into()),
            _ => {}
        }
    }
}
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
        let mut fields = Fields::default();
        event.record(&mut fields);
        if let (Some(operator), Some(path), Some(state)) =
            (fields.operator, fields.path, fields.state)
        {
            self.0.lock().expect("path events").push(Event {
                operator,
                path,
                state,
            });
        }
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

/// Captures each completed operator on the executing caller thread.
///
/// # Panics
/// Propagates the selected query panic or a poisoned event lock.
pub fn capture<T>(run: impl FnOnce() -> T) -> (T, Vec<Event>) {
    let capture = Capture::default();
    let result = tracing::subscriber::with_default(capture.clone(), run);
    let events = std::mem::take(&mut *capture.0.lock().expect("path events"));
    (result, events)
}
