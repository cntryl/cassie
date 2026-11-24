//! Checked serialized-size accounting for aggregate entry slices.
use crate::executor::QueryError;
use crate::runtime::QueryExecutionControls;
use crate::types::Value;

#[path = "group_array_size.rs"]
mod group_array_size;
#[cfg(test)]
#[path = "group_size_tests.rs"]
mod tests;

pub(super) fn json_bytes<T: serde::Serialize + ?Sized>(value: &T) -> usize {
    serde_json::to_vec(value).map_or_default(|bytes| bytes.len())
}

pub(super) fn json_bytes_controlled(
    values: &[(String, Value)],
    controls: &QueryExecutionControls,
) -> Result<usize, QueryError> {
    json_bytes_controlled_observing(values, controls, || {
        #[cfg(test)]
        NEXT_COUNT_CANCEL.with(|slot| {
            if let Some(cancel) = slot.borrow_mut().take() {
                cancel.cancel();
            }
        });
    })
}

#[cfg(test)]
thread_local! {
    static NEXT_COUNT_CANCEL: std::cell::RefCell<Option<crate::runtime::QueryCancellationHandle>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn cancel_next_chunk(cancel: crate::runtime::QueryCancellationHandle) {
    NEXT_COUNT_CANCEL.with(|slot| *slot.borrow_mut() = Some(cancel));
}

#[cfg(test)]
pub(super) fn clear_next_chunk() {
    NEXT_COUNT_CANCEL.with(|slot| *slot.borrow_mut() = None);
}

fn json_bytes_controlled_observing(
    values: &[(String, Value)],
    controls: &QueryExecutionControls,
    after_chunk: impl FnMut(),
) -> Result<usize, QueryError> {
    json_bytes_with_observers(values, controls, || {}, after_chunk)
}

fn json_bytes_with_observers(
    values: &[(String, Value)],
    controls: &QueryExecutionControls,
    mut after_entry: impl FnMut(),
    mut after_chunk: impl FnMut(),
) -> Result<usize, QueryError> {
    // Preserve the generic serde path for variants outside this explicit proof.
    for (_, value) in values {
        controls.check_interruption()?;
        if !matches!(
            value,
            Value::Null
                | Value::Bool(_)
                | Value::Int64(_)
                | Value::Float64(_)
                | Value::String(_)
                | Value::Json(_)
                | Value::Vector(_)
                | Value::Array(_)
        ) && !matches!(value, Value::Typed(typed) if supported_typed(typed))
        {
            return Ok(json_bytes(values));
        }
        after_entry();
    }
    let mut bytes = 2_usize; // outer list brackets
    for (index, (name, value)) in values.iter().enumerate() {
        controls.check_interruption()?;
        bytes = bytes
            .saturating_add(usize::from(index != 0))
            .saturating_add(2); // tuple and comma
        bytes = bytes
            .saturating_add(2)
            .saturating_add(escaped_payload_bytes_observing(
                name,
                controls,
                &mut after_chunk,
            )?);
        bytes = bytes.saturating_add(1); // tuple separator
        let value_bytes = if let Value::Array(array) = value {
            match group_array_size::count(array, controls, &mut after_chunk)? {
                group_array_size::ArraySize::Bytes(bytes) => Some(bytes),
                group_array_size::ArraySize::Unsupported => return Ok(json_bytes(values)),
                group_array_size::ArraySize::SerializationFailed => None,
            }
        } else {
            non_array_value_bytes(value, controls, &mut after_chunk)?
        };
        let Some(value_bytes) = value_bytes else {
            return Ok(0);
        };
        bytes = bytes.saturating_add(value_bytes);
    }
    controls.check_interruption()?;
    Ok(bytes)
}

fn non_array_value_bytes(
    value: &Value,
    controls: &QueryExecutionControls,
    after_chunk: &mut impl FnMut(),
) -> Result<Option<usize>, QueryError> {
    Ok(match value {
        Value::Null => Some("\"Null\"".len()),
        Value::Bool(value) => Some("{\"Bool\":".len() + (if *value { 4 } else { 5 }) + 1),
        Value::Int64(value) => scalar_json_bytes(value)
            .map(|size| "{\"Int64\":".len().saturating_add(size).saturating_add(1)),
        Value::Float64(value) => scalar_json_bytes(value)
            .map(|size| "{\"Float64\":".len().saturating_add(size).saturating_add(1)),
        Value::String(text) => Some(
            "{\"String\":"
                .len()
                .saturating_add(2)
                .saturating_add(escaped_payload_bytes_observing(
                    text,
                    controls,
                    after_chunk,
                )?)
                .saturating_add(1),
        ),
        Value::Json(document) => {
            #[cfg(not(test))]
            let size = crate::runtime::accounted::compact_json::count(document, controls)?;
            #[cfg(test)]
            let size = crate::runtime::accounted::compact_json::count_observing(
                document,
                controls,
                |progress| {
                    if matches!(
                        progress,
                        crate::runtime::accounted::compact_json::Progress::Scan
                    ) {
                        after_chunk();
                    }
                },
            )?;
            Some("{\"Json\":".len().saturating_add(size).saturating_add(1))
        }
        Value::Vector(vector) => vector_json_bytes(&vector.values, controls, after_chunk)?,
        Value::Typed(crate::types::typed::TypedValue::Bit(bits)) => {
            bit_json_bytes(bits, false, controls, after_chunk)?
        }
        Value::Typed(crate::types::typed::TypedValue::VarBit(bits)) => {
            bit_json_bytes(bits, true, controls, after_chunk)?
        }
        Value::Typed(crate::types::typed::TypedValue::Numeric(numeric)) => {
            let text = crate::executor::numeric_controlled_text::format(numeric, controls)?;
            Some(
                "{\"Typed\":{\"Numeric\":\""
                    .len()
                    .saturating_add(text.text().len())
                    .saturating_add("\"}}".len()),
            )
        }
        Value::Typed(typed) if fixed_typed(typed) => {
            controls.check_interruption()?;
            let size = scalar_json_bytes(value);
            controls.check_interruption()?;
            size
        }
        Value::Array(_) | Value::Typed(_) => unreachable!("non-array eligibility checked"),
    })
}

fn supported_typed(value: &crate::types::typed::TypedValue) -> bool {
    fixed_typed(value)
        || matches!(
            value,
            crate::types::typed::TypedValue::Bit(_)
                | crate::types::typed::TypedValue::VarBit(_)
                | crate::types::typed::TypedValue::Numeric(_)
        )
}

fn bit_json_bytes(
    bits: &crate::types::extended::bit_string::BitString,
    varying: bool,
    controls: &QueryExecutionControls,
    after_chunk: &mut impl FnMut(),
) -> Result<Option<usize>, QueryError> {
    controls.check_interruption()?;
    let Some(length) = scalar_json_bytes(&bits.len()) else {
        return Ok(None);
    };
    let mut bytes = if varying {
        "{\"Typed\":{\"VarBit\":{\"len\":".len()
    } else {
        "{\"Typed\":{\"Bit\":{\"len\":".len()
    };
    bytes = bytes
        .saturating_add(length)
        .saturating_add(",\"bytes\":[".len());
    for (index, value) in bits.bytes().iter().enumerate() {
        controls.check_interruption()?;
        let Some(size) = scalar_json_bytes(value) else {
            return Ok(None);
        };
        bytes = bytes
            .saturating_add(usize::from(index != 0))
            .saturating_add(size);
        if index.wrapping_add(1).is_multiple_of(256) {
            after_chunk();
            controls.check_interruption()?;
        }
    }
    controls.check_interruption()?;
    Ok(Some(bytes.saturating_add("]}}}".len())))
}

fn fixed_typed(value: &crate::types::typed::TypedValue) -> bool {
    use crate::types::typed::TypedValue;
    match value {
        TypedValue::Real(_)
        | TypedValue::Inet(_)
        | TypedValue::Cidr(_)
        | TypedValue::Macaddr(_)
        | TypedValue::Macaddr8(_)
        | TypedValue::PgLsn(_)
        | TypedValue::Date(_)
        | TypedValue::Time(_)
        | TypedValue::TimeTz(_)
        | TypedValue::Timestamp(_)
        | TypedValue::TimestampTz(_)
        | TypedValue::Interval(_) => true,
        TypedValue::Numeric(_)
        | TypedValue::Bit(_)
        | TypedValue::VarBit(_)
        | TypedValue::Record(_)
        | TypedValue::Enum(_) => false,
    }
}

fn vector_json_bytes(
    values: &[f32],
    controls: &QueryExecutionControls,
    after_chunk: &mut impl FnMut(),
) -> Result<Option<usize>, QueryError> {
    let mut bytes = "{\"Vector\":{\"values\":[".len();
    for (index, value) in values.iter().enumerate() {
        controls.check_interruption()?;
        let Some(size) = scalar_json_bytes(value) else {
            return Ok(None);
        };
        bytes = bytes
            .saturating_add(usize::from(index != 0))
            .saturating_add(size);
        if index.wrapping_add(1).is_multiple_of(256) {
            after_chunk();
            controls.check_interruption()?;
        }
    }
    controls.check_interruption()?;
    Ok(Some(bytes.saturating_add("]}}".len())))
}

fn scalar_json_bytes(value: &impl serde::Serialize) -> Option<usize> {
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .ok_or_else(|| std::io::Error::other("JSON size overflow"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter(0);
    serde_json::to_writer(&mut counter, value).ok()?;
    Some(counter.0)
}

pub(super) fn escaped_payload_bytes_observing(
    value: &str,
    controls: &QueryExecutionControls,
    mut after_chunk: impl FnMut(),
) -> Result<usize, QueryError> {
    let mut length = 0_usize;
    for (index, byte) in value.bytes().enumerate() {
        if index.is_multiple_of(1024) {
            if index != 0 {
                after_chunk();
            }
            controls.check_interruption()?;
        }
        let encoded = match byte {
            b'"' | b'\\' | b'\x08' | b'\x0c' | b'\n' | b'\r' | b'\t' => 2,
            0..=0x1f => 6,
            _ => 1,
        };
        length = crate::executor::retained_memory::add(length, encoded)?;
    }
    controls.check_interruption()?;
    Ok(length)
}
