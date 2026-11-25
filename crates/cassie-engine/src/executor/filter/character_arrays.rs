//! Native character-array casts preserve shape and admit element expansion.
use super::{casts, scalar_from_value, DataType, EvaluationMemory, QueryError, ScalarValue};
use crate::app::CassieError;
use crate::executor::retained_memory::{add, mul, value_clone_bytes};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::{SqlArray, Value};
use std::mem::size_of;

pub(super) fn cast(
    value: &ScalarValue,
    target: &DataType,
    source: Option<&DataType>,
    controls: Option<&QueryExecutionControls>,
    memory: Option<&EvaluationMemory>,
    explicit: bool,
) -> Option<Result<ScalarValue, QueryError>> {
    let DataType::Array(element) = target else {
        return None;
    };
    let target_element = base_element(element);
    if !matches!(
        target_element,
        DataType::Text | DataType::Char { .. } | DataType::Varchar { .. }
    ) {
        return None;
    }
    Some(convert(
        value,
        target_element,
        source,
        controls,
        memory,
        explicit,
    ))
}

fn convert(
    value: &ScalarValue,
    target: &DataType,
    source: Option<&DataType>,
    controls: Option<&QueryExecutionControls>,
    memory: Option<&EvaluationMemory>,
    explicit: bool,
) -> Result<ScalarValue, QueryError> {
    check(controls)?;
    if matches!(value, ScalarValue::Null) {
        return Ok(ScalarValue::Null);
    }
    let source_element = source.and_then(|kind| match kind {
        DataType::Array(element) => Some(base_element(element)),
        _ => None,
    });
    if source_element.is_some_and(|kind| matches!(kind, DataType::Vector(_) | DataType::JsonPath)) {
        return Err(CassieError::Unsupported(
            "this source array element family is not selected for character conversion".into(),
        )
        .into());
    }
    let mut admission = Admission::new(controls, memory)?;
    let parsed;
    let array = match value {
        ScalarValue::Array(array) => array,
        ScalarValue::Str(text) => {
            admission.grow(add(mul(text.len(), 128)?, 1024)?)?;
            parsed = super::text_arrays::cast(value, &DataType::Text)?;
            let ScalarValue::Array(array) = &parsed else {
                unreachable!("array text parser returns an array");
            };
            array
        }
        _ => {
            return Err(QueryError::General(
                "cannot cast scalar value to ARRAY".into(),
            ))
        }
    };
    admission.grow(scratch_bytes(array, target, controls)?)?;
    let mut elements = Vec::new();
    elements
        .try_reserve_exact(array.len())
        .map_err(|error| allocation(&error))?;
    if elements.capacity() > array.len() {
        return Err(resource(
            "allocator exceeded admitted character array capacity",
        ));
    }
    for value in array.elements() {
        check(controls)?;
        if value.is_null() {
            elements.push(Value::Null);
            continue;
        }
        #[cfg(test)]
        CONVERTED_ELEMENTS.with(|count| count.set(count.get() + 1));
        let scalar = scalar_from_value(value);
        let converted = if explicit {
            casts::scalar(&scalar, target, source_element, controls)?
        } else {
            let text = casts::scalar(&scalar, &DataType::Text, source_element, controls)?;
            if let ScalarValue::Str(text) = &text {
                let (width, padded) = modifier(target);
                crate::types::char_text::conversion_plan(text, width, padded, false)
                    .map_err(CassieError::TypeInput)?;
            }
            casts::scalar(&text, target, Some(&DataType::Text), controls)?
        };
        elements.push(converted.to_value());
        #[cfg(test)]
        CANCEL_AFTER_ELEMENT.with(|slot| {
            if let Some(cancellation) = slot.borrow_mut().take() {
                cancellation.cancel();
            }
        });
    }
    check(controls)?;
    SqlArray::new(array.dimensions().to_vec(), elements)
        .map(ScalarValue::Array)
        .map_err(QueryError::General)
}

fn scratch_bytes(
    array: &SqlArray,
    target: &DataType,
    controls: Option<&QueryExecutionControls>,
) -> Result<usize, QueryError> {
    let mut bytes = add(
        mul(
            array.dimensions().len(),
            size_of::<crate::types::ArrayDimension>(),
        )?,
        mul(array.len(), size_of::<Value>())?,
    )?;
    let session = super::temporal::session_of(controls);
    for value in array.elements() {
        check(controls)?;
        if value.is_null() {
            continue;
        }
        let text = match value {
            Value::String(text) => text.capacity(),
            Value::Typed(value) => crate::executor::native_result_text_bound(value, &session)?,
            Value::Json(value) => crate::executor::retained_memory::serialized_json_bytes(value)?,
            Value::Bool(_) | Value::Int64(_) | Value::Float64(_) => 64,
            Value::Array(_) | Value::Vector(_) => {
                return Err(CassieError::Unsupported(
                    "this source array element is not a selected scalar".into(),
                )
                .into())
            }
            Value::Null => 0,
        };
        let output = match target {
            DataType::Char {
                length: Some(width),
            } => text.max(mul(
                usize::try_from(*width).map_err(|_| resource("character width overflow"))?,
                4,
            )?),
            _ => text,
        };
        bytes = add(
            bytes,
            add(
                mul(value_clone_bytes(value)?, 4)?,
                mul(add(text, output)?, 4)?,
            )?,
        )?;
    }
    Ok(bytes)
}

pub(crate) fn validate_input(
    text: &str,
    target: &DataType,
    controls: Option<&QueryExecutionControls>,
) -> Option<Result<(), QueryError>> {
    let DataType::Array(element) = target else {
        return None;
    };
    let element = base_element(element);
    if !matches!(element, DataType::Char { .. } | DataType::Varchar { .. }) {
        return None;
    }
    Some((|| {
        check(controls)?;
        let _memory = controls
            .map(|controls| controls.reserve_query_memory(add(mul(text.len(), 128)?, 1024)?))
            .transpose()?;
        let parsed = super::text_arrays::cast(&ScalarValue::Str(text.to_owned()), &DataType::Text)?;
        let ScalarValue::Array(array) = parsed else {
            unreachable!("array parser result");
        };
        let (width, padded) = modifier(element);
        for value in array.elements() {
            check(controls)?;
            if let Value::String(value) = value {
                crate::types::char_text::conversion_plan(value, width, padded, false)
                    .map_err(CassieError::TypeInput)?;
            }
        }
        Ok(())
    })())
}

fn base_element(mut kind: &DataType) -> &DataType {
    while let DataType::Array(element) = kind {
        kind = element;
    }
    kind
}

fn modifier(kind: &DataType) -> (Option<u32>, bool) {
    match kind {
        DataType::Char { length } => (*length, true),
        DataType::Varchar { length } => (*length, false),
        _ => (None, false),
    }
}

struct Admission<'a> {
    memory: Option<&'a EvaluationMemory>,
    temporary: Option<QueryMemoryReservation>,
}
impl<'a> Admission<'a> {
    fn new(
        controls: Option<&QueryExecutionControls>,
        memory: Option<&'a EvaluationMemory>,
    ) -> Result<Self, QueryError> {
        Ok(Self {
            memory,
            temporary: if memory.is_none() {
                controls
                    .map(|controls| controls.reserve_query_memory(0))
                    .transpose()?
            } else {
                None
            },
        })
    }
    fn grow(&mut self, bytes: usize) -> Result<(), QueryError> {
        if let Some(memory) = self.memory {
            memory.grow(bytes)?;
        } else if let Some(memory) = &mut self.temporary {
            memory.try_grow(bytes)?;
        }
        Ok(())
    }
}
fn check(controls: Option<&QueryExecutionControls>) -> Result<(), QueryError> {
    controls.map_or(Ok(()), |controls| {
        controls.check_interruption().map_err(Into::into)
    })
}
fn allocation(error: &std::collections::TryReserveError) -> QueryError {
    resource(&format!("unable to allocate character array: {error}"))
}
fn resource(message: &str) -> QueryError {
    CassieError::ResourceLimit(message.to_owned()).into()
}

#[cfg(test)]
thread_local! { static CONVERTED_ELEMENTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
#[cfg(test)]
thread_local! { static CANCEL_AFTER_ELEMENT: std::cell::RefCell<Option<crate::runtime::QueryCancellationHandle>> = const { std::cell::RefCell::new(None) }; }
#[cfg(test)]
#[path = "character_arrays_tests.rs"]
mod tests;
