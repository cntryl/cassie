//! Temporary, independently owned JSON operands for controlled expressions.
use super::{as_json_document, QueryError, ScalarValue};
use crate::runtime::accounted::compact_json::{Progress, Sink};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

#[cfg(test)]
#[path = "json_operand_tests.rs"]
mod tests;

#[cfg(test)]
thread_local! {
    static CANCEL_COPY: std::cell::RefCell<Option<(crate::runtime::QueryCancellationHandle, usize)>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn cancel_copy_once(handle: crate::runtime::QueryCancellationHandle, number: usize) {
    CANCEL_COPY.with(|slot| *slot.borrow_mut() = Some((handle, number)));
}

pub(in crate::executor::filter) struct OwnedOperand {
    value: ScalarValue,
    _memory: Option<QueryMemoryReservation>,
}

impl OwnedOperand {
    pub(in crate::executor::filter) fn legacy(value: ScalarValue) -> Self {
        Self {
            value,
            _memory: None,
        }
    }

    pub(in crate::executor::filter) fn value(&self) -> &ScalarValue {
        &self.value
    }

    pub(in crate::executor::filter) fn convert(
        value: ScalarValue,
        controls: &QueryExecutionControls,
    ) -> Result<Self, QueryError> {
        match value {
            value @ (ScalarValue::Null | ScalarValue::Json(_)) => Ok(Self::legacy(value)),
            ScalarValue::Str(text) => {
                let mut sink = Sink::for_arena(controls)?;
                sink.quoted(&text, &mut progress)?;
                Ok(Self::from_sink(sink))
            }
            ScalarValue::Bool(value) => {
                let mut sink = Sink::for_arena(controls)?;
                sink.write(if value { "true" } else { "false" }, &mut progress)?;
                Ok(Self::from_sink(sink))
            }
            ScalarValue::Int(value) => Self::number(&value, controls),
            ScalarValue::Float(value) if value.is_finite() => Self::number(&value, controls),
            other => Ok(Self::legacy(as_json_document(&other)?)),
        }
    }

    fn number(
        value: &impl serde::Serialize,
        controls: &QueryExecutionControls,
    ) -> Result<Self, QueryError> {
        let mut buffer = [0_u8; 64];
        let length = {
            let mut cursor = std::io::Cursor::new(&mut buffer[..]);
            serde_json::to_writer(&mut cursor, value)
                .map_err(|error| QueryError::General(error.to_string()))?;
            usize::try_from(cursor.position())
                .map_err(|_| QueryError::General("JSON scalar length overflow".into()))?
        };
        let text = std::str::from_utf8(&buffer[..length])
            .map_err(|_| QueryError::General("invalid JSON scalar encoding".into()))?;
        let mut sink = Sink::for_arena(controls)?;
        sink.write(text, &mut progress)?;
        Ok(Self::from_sink(sink))
    }

    fn from_sink(sink: Sink<'_>) -> Self {
        let (text, memory) = sink.finish_output().into_parts();
        Self {
            value: ScalarValue::Json(text),
            _memory: Some(memory),
        }
    }
}

fn progress(progress: Progress) {
    #[cfg(test)]
    if matches!(progress, Progress::Copy) {
        CANCEL_COPY.with(|slot| {
            let mut slot = slot.borrow_mut();
            if let Some((handle, remaining)) = slot.as_mut() {
                *remaining -= 1;
                if *remaining == 0 {
                    handle.cancel();
                    *slot = None;
                }
            }
        });
    }
    #[cfg(not(test))]
    let _ = progress;
}
