//! Decoding of `Bind` parameter values, including parameters typed by a named
//! enum or enum array OID (#827).
//!
//! User-range OIDs have no builtin codec, so enum parameters bypass it: text
//! and binary input both carry label text, which the application layer parses
//! against the enum descriptor (`22P02` / `55P04`).
use super::super::codecs::enum_value;
use super::text_parameters::DecodedTextParameter;
use super::{
    decode_parameter_with_vector, format_for_index, Cassie, CassieSession, ExtendedQueryError,
};
use crate::app::enum_parameters::EnumParameter;
use crate::executor::EnumColumn;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::Value;

/// The enum description of each parameter slot whose OID names an enum type.
/// Empty when the catalog has no enum types.
pub(super) fn enum_columns(
    cassie: &Cassie,
    session: &CassieSession,
    parameter_types: &[i32],
) -> Vec<Option<EnumColumn>> {
    if !cassie.catalog.has_enum_types() {
        return Vec::new();
    }
    let database = session
        .current_database()
        .unwrap_or(&cassie.default_database);
    parameter_types
        .iter()
        .map(|oid| {
            EnumParameter::for_oid(&cassie.catalog, database, *oid)
                .map(|parameter| parameter.column())
        })
        .collect()
}

/// Decodes every bound parameter. `wire` is the `(values, formats, declared
/// OIDs)` triple of the `Bind` message; `special` is the session's vector OID
/// and the enum description of each slot.
pub(super) fn decode(
    wire: (&[Option<Vec<u8>>], &[i16], &[i32]),
    controls: &QueryExecutionControls,
    special: (Option<i32>, &[Option<EnumColumn>]),
) -> Result<(Vec<Value>, Vec<QueryMemoryReservation>), ExtendedQueryError> {
    let (parameters, parameter_formats, parameter_types) = wire;
    let (vector_oid, enum_columns) = special;
    let decoded_parameters = parameters
        .iter()
        .enumerate()
        .map(|(index, parameter)| {
            let format = format_for_index(parameter_formats, index);
            if let Some(column) = enum_columns.get(index).copied().flatten() {
                return decode_enum(parameter.as_deref(), format, column);
            }
            decode_parameter_with_vector(
                parameter.as_deref(),
                format,
                parameter_types[index],
                index,
                controls,
                vector_oid.is_some_and(|vector_oid| parameter_types[index] == vector_oid),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let (values, parameter_memory): (Vec<_>, Vec<_>) = decoded_parameters
        .into_iter()
        .map(|parameter| (parameter.value, parameter.memory))
        .unzip();
    let parameter_memory = parameter_memory.into_iter().flatten().collect();
    Ok((values, parameter_memory))
}

/// Decodes one enum parameter to label text, or an array of label text.
fn decode_enum(
    parameter: Option<&[u8]>,
    format: i16,
    column: EnumColumn,
) -> Result<DecodedTextParameter, ExtendedQueryError> {
    let Some(parameter) = parameter else {
        return Ok(DecodedTextParameter::unaccounted(Value::Null));
    };
    let value = if format == 0 {
        std::str::from_utf8(parameter)
            .map(|text| Value::String(text.to_string()))
            .map_err(|_| ExtendedQueryError::protocol("invalid enum bind parameter"))?
    } else {
        enum_value::decode_labels(parameter, column)
            .map_err(|error| ExtendedQueryError::protocol_from_io(&error))?
    };
    Ok(DecodedTextParameter::unaccounted(value))
}
