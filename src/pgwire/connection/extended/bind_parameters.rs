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
fn enum_columns(
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

/// The `(values, formats, declared OIDs)` triple of a `Bind` message.
pub(super) type WireParameters<'a> = (&'a [Option<Vec<u8>>], &'a [i16], &'a [i32]);

/// A parameter slot typed by a user-range OID, which has no builtin codec.
#[derive(Debug, Clone)]
pub(super) enum UserParameter {
    Enum(EnumColumn),
    Record(super::record_parameters::RecordParameter),
}

/// The session's vector OID and the user-type description of each parameter slot.
pub(super) type SpecialParameterTypes<'a> = (Option<i32>, &'a [Option<UserParameter>]);

/// The user-type description of each parameter slot: an enum, a composite or
/// an array of either. Empty when no slot names such a type.
pub(super) fn user_parameters(
    cassie: &Cassie,
    session: &CassieSession,
    parameter_types: &[i32],
) -> Vec<Option<UserParameter>> {
    let enums = enum_columns(cassie, session, parameter_types);
    let records = super::record_parameters::for_slots(cassie, session, parameter_types);
    if enums.is_empty() && records.is_empty() {
        return Vec::new();
    }
    (0..parameter_types.len())
        .map(|index| {
            enums
                .get(index)
                .copied()
                .flatten()
                .map(UserParameter::Enum)
                .or_else(|| {
                    records
                        .get(index)
                        .cloned()
                        .flatten()
                        .map(UserParameter::Record)
                })
        })
        .collect()
}

/// Decodes every bound parameter. `wire` is the `(values, formats, declared
/// OIDs)` triple of the `Bind` message; `special` is the session's vector OID
/// and the enum description of each slot.
pub(super) fn decode(
    wire: WireParameters<'_>,
    controls: &QueryExecutionControls,
    special: SpecialParameterTypes<'_>,
) -> Result<(Vec<Value>, Vec<QueryMemoryReservation>), ExtendedQueryError> {
    let (parameters, parameter_formats, parameter_types) = wire;
    let (vector_oid, user_parameters) = special;
    let decoded_parameters = parameters
        .iter()
        .enumerate()
        .map(|(index, parameter)| {
            let format = format_for_index(parameter_formats, index);
            match user_parameters.get(index).and_then(Option::as_ref) {
                Some(UserParameter::Enum(column)) => {
                    return decode_enum(parameter.as_deref(), format, *column);
                }
                Some(UserParameter::Record(record)) => {
                    return super::record_parameters::decode(
                        parameter.as_deref(),
                        format,
                        record,
                        index,
                    );
                }
                None => {}
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
