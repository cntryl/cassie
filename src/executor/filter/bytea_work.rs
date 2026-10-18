//! Scratch admission shared by codecs, bit scans and hash updates.
use super::{QueryError, Value};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

pub(super) struct Work<'a> {
    controls: Option<&'a QueryExecutionControls>,
    _memory: Option<QueryMemoryReservation>,
}
impl<'a> Work<'a> {
    pub(super) fn admit(
        name: &str,
        args: &[Value],
        controls: Option<&'a QueryExecutionControls>,
    ) -> Result<Self, QueryError> {
        if let Some(controls) = controls {
            controls.check_interruption()?;
        }
        // Arguments already belong to their caller. Admit only the decoded
        // buffer and this operation's simultaneously live output allocation.
        // Output collectors separately own retained result memory.
        let input = scratch_bytes(name, args).ok_or_else(|| {
            QueryError::Cassie(crate::app::CassieError::ResourceLimit(
                "BYTEA function scratch overflow".into(),
            ))
        })?;
        let memory = controls
            .map(|controls| controls.reserve_query_memory(input))
            .transpose()?;
        Ok(Self {
            controls,
            _memory: memory,
        })
    }
    pub(super) fn check(&self) -> Result<(), QueryError> {
        self.controls
            .map_or(Ok(()), QueryExecutionControls::check_interruption)
            .map_err(Into::into)
    }
}

fn scratch_bytes(name: &str, args: &[Value]) -> Option<usize> {
    let input = args.first().and_then(Value::as_str).unwrap_or_default();
    let format = args.get(1).and_then(Value::as_str).unwrap_or_default();
    let decoded = if name == "decode" {
        if format.eq_ignore_ascii_case("hex") {
            input.len() / 2
        } else if format.eq_ignore_ascii_case("escape") {
            input.len()
        } else if format.eq_ignore_ascii_case("base64") {
            (input.len() / 4).checked_mul(3)?
        } else {
            0
        }
    } else {
        input.len().saturating_sub(2) / 2
    };
    let output = match name {
        "decode" | "encode"
            if !["hex", "escape", "base64"]
                .iter()
                .any(|encoding| format.eq_ignore_ascii_case(encoding)) =>
        {
            encoding_error_capacity(format)?
        }
        "decode" | "set_byte" | "set_bit" => decoded.checked_mul(2)?.checked_add(2)?,
        "sha224" => 58,
        "sha256" => 66,
        "sha384" => 98,
        "sha512" => 130,
        "encode" if format.eq_ignore_ascii_case("hex") => decoded.checked_mul(2)?,
        "encode" if format.eq_ignore_ascii_case("escape") => decoded.checked_mul(4)?,
        "encode" if format.eq_ignore_ascii_case("base64") => base64_capacity(decoded)?,
        _ => 0,
    };
    decoded.checked_add(output)
}

pub(super) fn base64_capacity(bytes: usize) -> Option<usize> {
    bytes.div_ceil(3).checked_mul(4)?.checked_add(bytes / 57)
}

pub(super) const ENCODING_ERROR_PREFIX: &str = "unrecognized encoding: \"";

pub(super) fn encoding_error_capacity(format: &str) -> Option<usize> {
    ENCODING_ERROR_PREFIX
        .len()
        .checked_add(format.len())?
        .checked_add(1)
}
