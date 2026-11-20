//! Plain column lanes shared by storage readers and the typed executor.
//!
//! This module depends only on [`Value`], so a storage adapter can produce
//! lanes without importing any executor type. The executor turns them into
//! typed columns with `Column::from_lanes`.
use crate::types::Value;

/// Per-lane validity. A set flag or bit means the lane holds a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LaneValidity {
    /// Every lane is valid (a `NOT NULL` column carries no flags).
    AllValid,
    /// One flag per lane.
    Flags(Vec<bool>),
    /// Little-endian bit words: lane `n` is bit `n % 64` of word `n / 64`.
    /// Bits at or beyond the lane count must be zero.
    Words(Vec<u64>),
}

/// One column's lanes in their storage-native carrier.
///
/// Lanes under an invalid flag hold an arbitrary placeholder that readers
/// never observe. `Values` is the generic carrier for every other logical
/// type; a `Value::Null` lane is its own invalid flag.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RawLanes {
    I64 {
        values: Vec<i64>,
        validity: LaneValidity,
    },
    F64 {
        values: Vec<f64>,
        validity: LaneValidity,
    },
    Bool {
        values: Vec<u8>,
        validity: LaneValidity,
    },
    /// UTF-8 text: lane `n` is `bytes[offsets[n]..offsets[n + 1]]`.
    Utf8 {
        bytes: String,
        offsets: Vec<usize>,
        validity: LaneValidity,
    },
    Values(Vec<Value>),
}

impl RawLanes {
    /// The number of lanes this carrier describes.
    pub(crate) fn len(&self) -> usize {
        match self {
            Self::I64 { values, .. } => values.len(),
            Self::F64 { values, .. } => values.len(),
            Self::Bool { values, .. } => values.len(),
            Self::Utf8 { offsets, .. } => offsets.len().saturating_sub(1),
            Self::Values(values) => values.len(),
        }
    }
}
