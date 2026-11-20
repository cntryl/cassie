//! Typed columns built from storage-native lanes instead of `Value` slices.
use std::mem::size_of;
use std::sync::Arc;

use super::{payload_bytes, validate_values, Column, ColumnData, Storage};
use crate::executor::retained_memory::{add, data_type_clone_bytes, mul};
use crate::executor::typed_batch::validity::Validity;
use crate::executor::typed_batch::{check_controls, invalid, owner_bytes, QueryError};
use crate::runtime::accounted::Accounted;
use crate::runtime::QueryExecutionControls;
use crate::types::lanes::{LaneValidity, RawLanes};
use crate::types::DataType;

impl Column {
    /// Admits and wraps lanes a storage reader already decoded.
    ///
    /// The retained bytes are accounted exactly like the plain (non-constant,
    /// non-sequence, non-dictionary) form `from_values` builds. The producer
    /// owns the allocation of `lanes` before admission; this call admits what
    /// the column retains and fails without retaining anything when the query
    /// budget cannot hold it.
    pub(crate) fn from_lanes(
        controls: &QueryExecutionControls,
        data_type: &DataType,
        lanes: RawLanes,
    ) -> Result<Self, QueryError> {
        check_controls(controls)?;
        let domain = lanes.len();
        let (validity, payload) = checked_parts(data_type, &lanes)?;
        let bitmap = if matches!(validity, Validity::Bitmap(_)) {
            mul(domain.div_ceil(64), size_of::<u64>())?
        } else {
            0
        };
        let bytes = owner_bytes::<ColumnData>(add(
            add(payload, bitmap)?,
            data_type_clone_bytes(data_type)?,
        )?)?;
        let owner = Accounted::try_new(controls, bytes, || ColumnData {
            data_type: data_type.clone(),
            domain,
            storage: storage(lanes),
            validity,
        })?;
        check_controls(controls)?;
        Ok(Self(Arc::new(owner)))
    }
}

fn storage(lanes: RawLanes) -> Storage {
    match lanes {
        RawLanes::I64 { values, .. } => Storage::Integer(values),
        RawLanes::F64 { values, .. } => Storage::Float(values),
        RawLanes::Bool { values, .. } => Storage::Boolean(values),
        RawLanes::Utf8 { bytes, offsets, .. } => Storage::Utf8 { bytes, offsets },
        RawLanes::Values(values) => Storage::Scalar(values),
    }
}

fn lane_validity(validity: &LaneValidity, domain: usize) -> Result<Validity, QueryError> {
    match validity {
        LaneValidity::AllValid => Ok(Validity::AllValid),
        LaneValidity::Flags(flags) if flags.len() == domain => Ok(Validity::from_flags(flags)),
        LaneValidity::Words(words) => Validity::from_words(words, domain)
            .ok_or_else(|| invalid("lane validity words disagree with lane count")),
        LaneValidity::Flags(_) => Err(invalid("lane validity flags disagree with lane count")),
    }
}

/// Validates `lanes` against `data_type`; returns the validity and payload bytes.
fn checked_parts(data_type: &DataType, lanes: &RawLanes) -> Result<(Validity, usize), QueryError> {
    let domain = lanes.len();
    let mismatch = || invalid("lane carrier does not match declared native transport");
    match lanes {
        RawLanes::I64 { values, validity } => {
            if !matches!(
                data_type,
                DataType::SmallInt | DataType::Int | DataType::BigInt
            ) {
                return Err(mismatch());
            }
            let validity = lane_validity(validity, domain)?;
            for (lane, value) in values.iter().enumerate() {
                let fits = match data_type {
                    DataType::SmallInt => i16::try_from(*value).is_ok(),
                    DataType::Int => i32::try_from(*value).is_ok(),
                    _ => true,
                };
                if !fits && validity.is_valid(lane) {
                    return Err(invalid("lane integer does not match declared transport"));
                }
            }
            Ok((validity, mul(domain, size_of::<i64>())?))
        }
        RawLanes::F64 { values, validity } => {
            if data_type != &DataType::Float {
                return Err(mismatch());
            }
            Ok((
                lane_validity(validity, domain)?,
                mul(values.len(), size_of::<f64>())?,
            ))
        }
        RawLanes::Bool { values, validity } => {
            if data_type != &DataType::Boolean {
                return Err(mismatch());
            }
            let validity = lane_validity(validity, domain)?;
            if values
                .iter()
                .enumerate()
                .any(|(lane, value)| *value > 1 && validity.is_valid(lane))
            {
                return Err(invalid("lane Boolean is not zero or one"));
            }
            Ok((validity, domain))
        }
        RawLanes::Utf8 {
            bytes,
            offsets,
            validity,
        } => {
            if !matches!(
                data_type,
                DataType::Text | DataType::Char { .. } | DataType::Varchar { .. }
            ) {
                return Err(mismatch());
            }
            let validity = lane_validity(validity, domain)?;
            let ordered = offsets.first() == Some(&0)
                && offsets.last() == Some(&bytes.len())
                && offsets.windows(2).all(|pair| pair[0] <= pair[1])
                && offsets.iter().all(|offset| bytes.is_char_boundary(*offset));
            if !ordered {
                return Err(invalid(
                    "lane text offsets are not ordered character bounds",
                ));
            }
            let payload = add(mul(offsets.len(), size_of::<usize>())?, bytes.len())?;
            Ok((validity, payload))
        }
        RawLanes::Values(values) => {
            validate_values(data_type, values)?;
            Ok((
                Validity::from_values(values),
                payload_bytes(data_type, values)?,
            ))
        }
    }
}

#[cfg(test)]
#[path = "lanes_tests.rs"]
mod tests;
