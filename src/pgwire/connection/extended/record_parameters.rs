//! `Bind` parameters typed by a named composite or composite array OID (#1268).
//!
//! User-range OIDs have no builtin codec. Text input is the record or array
//! literal itself. Binary input (`record_recv`, `array_recv`) is decoded field
//! by field and handed on as the equivalent literal, so both formats are
//! converted and validated by the one typed input path the SQL cast uses.
use super::super::codecs;
use super::text_parameters::DecodedTextParameter;
use super::{Cassie, CassieSession, ExtendedQueryError};
use crate::types::composite::{format_record_with, receive_record_binary, CompositeRef};
use crate::types::extended::temporal::default_session;
use crate::types::typed::TypedValue;
use crate::types::{ArrayDimension, DataType, RecordValue, SqlArray, Value};

/// A parameter slot whose OID names a composite type or its array type.
#[derive(Debug, Clone)]
pub(super) struct RecordParameter {
    descriptor: CompositeRef,
    is_array: bool,
}

impl RecordParameter {
    /// The record description of `oid` in `database`, if it names a composite
    /// type or the array type of one.
    pub(super) fn for_oid(cassie: &Cassie, database: &str, oid: i32) -> Option<Self> {
        let oid = u32::try_from(oid).ok()?;
        let (metadata, is_array) = cassie
            .catalog
            .get_composite_type_by_oid(oid)
            .map(|metadata| (metadata, false))
            .or_else(|| {
                cassie
                    .catalog
                    .get_composite_type_by_array_oid(oid)
                    .map(|metadata| (metadata, true))
            })?;
        metadata
            .database
            .eq_ignore_ascii_case(database)
            .then(|| Self {
                descriptor: metadata.descriptor(),
                is_array,
            })
    }
}

/// The record description of each parameter slot. Empty when the catalog has
/// no composite types.
pub(super) fn for_slots(
    cassie: &Cassie,
    session: &CassieSession,
    parameter_types: &[i32],
) -> Vec<Option<RecordParameter>> {
    if !cassie.catalog.has_composite_types() {
        return Vec::new();
    }
    let database = session
        .current_database()
        .unwrap_or(&cassie.default_database);
    parameter_types
        .iter()
        .map(|oid| RecordParameter::for_oid(cassie, database, *oid))
        .collect()
}

fn malformed(index: usize, detail: &str) -> ExtendedQueryError {
    let error: crate::app::CassieError = crate::app::DiagnosticError::new(
        "22P03",
        format!(
            "incorrect binary data format in bind parameter {}",
            index + 1
        ),
    )
    .with_detail(detail.to_owned())
    .into();
    ExtendedQueryError::cassie(&error)
}

/// Decodes one composite or composite array parameter to its literal text.
pub(super) fn decode(
    parameter: Option<&[u8]>,
    format: i16,
    record: &RecordParameter,
    index: usize,
) -> Result<DecodedTextParameter, ExtendedQueryError> {
    let Some(parameter) = parameter else {
        return Ok(DecodedTextParameter::unaccounted(Value::Null));
    };
    let text = if format == 0 {
        std::str::from_utf8(parameter)
            .map(str::to_owned)
            .map_err(|_| ExtendedQueryError::protocol("invalid composite bind parameter"))?
    } else if record.is_array {
        let array = receive_array(parameter, &record.descriptor)
            .map_err(|detail| malformed(index, &detail))?;
        array_text(&array).map_err(|detail| malformed(index, &detail))?
    } else {
        let value = receive_record(parameter, &record.descriptor)
            .map_err(|detail| malformed(index, &detail))?;
        record_text(&value)
    };
    validate_reserved(&text, record)?;
    Ok(DecodedTextParameter::unaccounted(Value::String(text)))
}

/// The reserved `cassie_graph` types carry invariants beyond their field
/// types, so a parameter of one is parsed and validated at `Bind`: malformed
/// text is `22P02` and a well-formed but invalid value is `22023`, in either
/// wire format. Any other composite stays a literal until it is cast.
fn validate_reserved(text: &str, record: &RecordParameter) -> Result<(), ExtendedQueryError> {
    if crate::types::graph::GraphTypeKind::of(&record.descriptor).is_none() {
        return Ok(());
    }
    let outcome = if record.is_array {
        crate::executor::scan::composite_arrays::from_text(text, &record.descriptor).map(drop)
    } else {
        crate::executor::filter::assign_record(&Value::String(text.to_owned()), &record.descriptor)
            .map(drop)
            .map_err(crate::app::CassieError::from)
    };
    outcome.map_err(|error| ExtendedQueryError::cassie(&error))
}

/// The element descriptor of an array-of-composite field type.
fn array_element(data_type: &DataType) -> Option<&CompositeRef> {
    match data_type {
        DataType::Array(inner) => match inner.as_ref() {
            DataType::Composite(descriptor) => Some(descriptor),
            _ => None,
        },
        _ => None,
    }
}

fn receive_record(bytes: &[u8], descriptor: &CompositeRef) -> Result<RecordValue, String> {
    receive_record_binary(bytes, descriptor, |data_type, field| {
        if let DataType::Composite(nested) = data_type {
            return receive_record(field, nested)
                .map(|record| Value::Typed(TypedValue::Record(record)));
        }
        if let Some(element) = array_element(data_type) {
            return receive_array(field, element).map(Value::Array);
        }
        codecs::binary_to_value_with_vector(field, data_type.type_oid(), false)
            .map_err(|error| error.to_string())
    })
}

fn read_i32(cursor: &mut &[u8]) -> Result<i32, String> {
    let Some((head, rest)) = cursor.split_first_chunk::<4>() else {
        return Err("insufficient data left in array message".to_owned());
    };
    *cursor = rest;
    Ok(i32::from_be_bytes(*head))
}

/// `array_recv` over `record_recv` elements of `descriptor`.
fn receive_array(bytes: &[u8], descriptor: &CompositeRef) -> Result<SqlArray, String> {
    const MAX_DIMENSIONS: i32 = 6;
    let mut cursor = bytes;
    let dimension_count = read_i32(&mut cursor)?;
    if !(0..=MAX_DIMENSIONS).contains(&dimension_count) {
        return Err(format!("invalid number of dimensions: {dimension_count}"));
    }
    let _has_null = read_i32(&mut cursor)?;
    let element_oid = read_i32(&mut cursor)?;
    if u32::try_from(element_oid).ok() != Some(descriptor.oid) {
        return Err(format!(
            "wrong element type: expected {}, received {element_oid}",
            descriptor.oid
        ));
    }
    let mut dimensions = Vec::new();
    let mut count = usize::from(dimension_count > 0);
    for _ in 0..dimension_count {
        let length = read_i32(&mut cursor)?;
        let lower_bound = read_i32(&mut cursor)?;
        let size = usize::try_from(length).map_err(|_| "invalid array dimension".to_owned())?;
        count = count
            .checked_mul(size)
            .ok_or_else(|| "array size exceeds the maximum allowed".to_owned())?;
        dimensions.push(ArrayDimension {
            length,
            lower_bound,
        });
    }
    // Every element needs at least its four-byte length.
    if count > cursor.len() / 4 {
        return Err("insufficient data left in array message".to_owned());
    }
    let mut elements = Vec::with_capacity(count);
    for _ in 0..count {
        let length = read_i32(&mut cursor)?;
        if length == -1 {
            elements.push(Value::Null);
            continue;
        }
        let length = usize::try_from(length)
            .ok()
            .filter(|length| *length <= cursor.len())
            .ok_or_else(|| "insufficient data left in array message".to_owned())?;
        let (element, rest) = cursor.split_at(length);
        cursor = rest;
        elements.push(Value::Typed(TypedValue::Record(receive_record(
            element, descriptor,
        )?)));
    }
    if !cursor.is_empty() {
        return Err("improper binary format in array message".to_owned());
    }
    SqlArray::new(dimensions, elements)
}

fn array_text(array: &SqlArray) -> Result<String, String> {
    let rendered = retext(array)?;
    crate::types::array_value::format_text_in(&rendered, &DataType::Text, default_session())
        .ok_or_else(|| "composite array cannot be rendered as text".to_owned())
}

/// `array` with each record replaced by its literal text, so the array
/// formatter quotes every element exactly once.
fn retext(array: &SqlArray) -> Result<SqlArray, String> {
    let elements = array
        .elements()
        .iter()
        .map(|element| match element {
            Value::Typed(TypedValue::Record(record)) => Value::String(record_text(record)),
            other => other.clone(),
        })
        .collect();
    SqlArray::new(array.dimensions().to_vec(), elements)
}

/// The record literal of a decoded record, each field rendered by the wire
/// text codec of its own type.
fn record_text(record: &RecordValue) -> String {
    format_record_with(record, &mut |value, data_type| match (value, data_type) {
        (Value::Typed(TypedValue::Record(nested)), _) => record_text(nested),
        (Value::Array(array), field_type) if array_element(field_type).is_some() => {
            array_text(array).unwrap_or_default()
        }
        _ => codecs::value_to_text(value, data_type.type_oid()),
    })
}
