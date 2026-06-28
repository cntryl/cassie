#![allow(clippy::result_large_err)]

use super::*;
pub(super) fn validate_parameter_formats(
    parameter_formats: &[i16],
    parameter_count: usize,
) -> Result<(), PgWireError> {
    if !parameter_formats.is_empty()
        && parameter_formats.len() != 1
        && parameter_formats.len() != parameter_count
    {
        return Err(PgWireError::protocol("unsupported bind format count"));
    }
    validate_codes(parameter_formats, "unsupported bind format code")
}

pub(super) fn validate_bind_result_formats(result_formats: &[i16]) -> Result<(), PgWireError> {
    validate_codes(result_formats, "unsupported result format code")
}

pub(super) fn decode_bind_params(
    params: &[Option<Vec<u8>>],
    parameter_formats: &[i16],
    parameter_types: &[i32],
) -> Result<Vec<Value>, PgWireError> {
    let mut decoded = Vec::with_capacity(params.len());
    for (index, param) in params.iter().enumerate() {
        let format_code = parameter_format_for_index(parameter_formats, index);
        let type_oid = parameter_types.get(index).copied().unwrap_or(705);
        let value = parse_bind_param_value(param.as_deref(), format_code, type_oid)
            .map_err(|error| PgWireError::protocol(format!("invalid bind parameter: {error:?}")))?;
        decoded.push(value);
    }
    Ok(decoded)
}

fn validate_codes(codes: &[i16], message: &'static str) -> Result<(), PgWireError> {
    if codes.iter().all(|format| matches!(*format, 0 | 1)) {
        Ok(())
    } else {
        Err(PgWireError::protocol(message))
    }
}

fn parameter_format_for_index(parameter_formats: &[i16], index: usize) -> i16 {
    match parameter_formats.len() {
        0 => 0,
        1 => parameter_formats[0],
        _ => parameter_formats[index],
    }
}
