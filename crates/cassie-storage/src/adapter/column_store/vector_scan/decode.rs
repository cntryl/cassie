//! Lane builders and per-cell decoding of the column vector reader.
//!
//! A stored field value is decoded straight into its lane: integers, floats,
//! Booleans and text never become JSON, and every other type goes through the
//! same JSON-to-`Value` conversion the row path uses, one value per cell.

use crate::adapter::column_store::compression::expand_column_field;
use crate::adapter::column_store::validation::corrupt_row;
use crate::adapter::CassieError;
use crate::adapter::StorageValueHooks;
use crate::row_blob::{decode_compact_scalar, CompactScalar, RowFieldMeta};
use cassie_core::types::lanes::{LaneValidity, RawLanes};
use cassie_core::types::{DataType, Value};

/// The lane carrier a declared column type decodes into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LaneKind {
    Integer,
    Float,
    Boolean,
    Utf8,
    Values,
}

impl LaneKind {
    pub(crate) fn of(data_type: &DataType) -> Self {
        match data_type {
            DataType::SmallInt | DataType::Int | DataType::BigInt => Self::Integer,
            DataType::Float => Self::Float,
            DataType::Boolean => Self::Boolean,
            DataType::Text | DataType::Varchar { .. } | DataType::Char { .. } => Self::Utf8,
            _ => Self::Values,
        }
    }

    /// Bytes one lane of this carrier occupies before its payload.
    pub(crate) fn lane_bytes(self) -> usize {
        match self {
            Self::Integer | Self::Float => 9,
            Self::Boolean => 2,
            Self::Utf8 => std::mem::size_of::<usize>() + 1,
            Self::Values => std::mem::size_of::<Value>(),
        }
    }
}

/// One column's lanes while a batch is built.
pub(crate) enum LaneBuilder {
    Integer(Fixed<i64>),
    Float(Fixed<f64>),
    Boolean(Fixed<u8>),
    Utf8 {
        bytes: String,
        offsets: Vec<usize>,
        valid: Vec<bool>,
        nulls: usize,
    },
    Values(Vec<Value>),
}

/// Fixed-width lanes with validity kept as little-endian bit words.
pub(crate) struct Fixed<T> {
    values: Vec<T>,
    words: Vec<u64>,
    nulls: usize,
}

impl<T: Default> Fixed<T> {
    fn with_capacity(rows: usize) -> Self {
        Self {
            values: Vec::with_capacity(rows),
            words: Vec::with_capacity(rows.div_ceil(64)),
            nulls: 0,
        }
    }

    fn push(&mut self, value: T) {
        let lane = self.values.len();
        if lane.is_multiple_of(64) {
            self.words.push(0);
        }
        if let Some(word) = self.words.last_mut() {
            *word |= 1 << (lane % 64);
        }
        self.values.push(value);
    }

    fn push_null(&mut self) {
        if self.values.len().is_multiple_of(64) {
            self.words.push(0);
        }
        self.values.push(T::default());
        self.nulls += 1;
    }

    fn validity(self) -> (Vec<T>, LaneValidity) {
        let validity = if self.nulls == 0 {
            LaneValidity::AllValid
        } else {
            LaneValidity::Words(self.words)
        };
        (self.values, validity)
    }
}

impl LaneBuilder {
    pub(crate) fn new(kind: LaneKind, rows: usize) -> Self {
        match kind {
            LaneKind::Integer => Self::Integer(Fixed::with_capacity(rows)),
            LaneKind::Float => Self::Float(Fixed::with_capacity(rows)),
            LaneKind::Boolean => Self::Boolean(Fixed::with_capacity(rows)),
            LaneKind::Utf8 => {
                let mut offsets = Vec::with_capacity(rows + 1);
                offsets.push(0);
                Self::Utf8 {
                    bytes: String::new(),
                    offsets,
                    valid: Vec::with_capacity(rows),
                    nulls: 0,
                }
            }
            LaneKind::Values => Self::Values(Vec::with_capacity(rows)),
        }
    }

    /// A lane with no value: SQL NULL, or a field the row predates.
    pub(crate) fn push_null(&mut self) {
        match self {
            Self::Integer(lane) => lane.push_null(),
            Self::Float(lane) => lane.push_null(),
            Self::Boolean(lane) => lane.push_null(),
            Self::Utf8 {
                bytes,
                offsets,
                valid,
                nulls,
            } => {
                offsets.push(bytes.len());
                valid.push(false);
                *nulls += 1;
            }
            Self::Values(values) => values.push(Value::Null),
        }
    }

    fn push_cell(
        &mut self,
        cell: CompactScalar<'_>,
        data_type: &DataType,
        hooks: &StorageValueHooks,
    ) -> Result<(), CassieError> {
        match (&mut *self, cell) {
            (_, CompactScalar::Null) | (Self::Values(_), CompactScalar::Generic(None)) => {
                self.push_null();
            }
            (Self::Integer(lane), CompactScalar::Integer(value)) => lane.push(value),
            (Self::Float(lane), CompactScalar::Float(value)) => lane.push(value),
            (Self::Boolean(lane), CompactScalar::Boolean(value)) => lane.push(u8::from(value)),
            (
                Self::Utf8 {
                    bytes,
                    offsets,
                    valid,
                    ..
                },
                CompactScalar::Text(text),
            ) => {
                bytes.push_str(text);
                offsets.push(bytes.len());
                valid.push(true);
            }
            (Self::Values(values), CompactScalar::Generic(Some(json))) => {
                values.push((hooks.decode_typed_value)(&json, data_type)?);
            }
            _ => {
                return Err(CassieError::Parse(
                    "stored value does not match the lane carrier of its declared type".to_owned(),
                ))
            }
        }
        Ok(())
    }

    pub(crate) fn finish(self) -> RawLanes {
        match self {
            Self::Integer(lane) => {
                let (values, validity) = lane.validity();
                RawLanes::I64 { values, validity }
            }
            Self::Float(lane) => {
                let (values, validity) = lane.validity();
                RawLanes::F64 { values, validity }
            }
            Self::Boolean(lane) => {
                let (values, validity) = lane.validity();
                RawLanes::Bool { values, validity }
            }
            Self::Utf8 {
                bytes,
                offsets,
                valid,
                nulls,
            } => RawLanes::Utf8 {
                bytes,
                offsets,
                validity: if nulls == 0 {
                    LaneValidity::AllValid
                } else {
                    LaneValidity::Flags(valid)
                },
            },
            Self::Values(values) => RawLanes::Values(values),
        }
    }
}

/// Decodes one stored field payload into `lane`, mapping a decode failure to
/// `XX001` exactly as `validation::decode_field` does.
///
/// # Errors
///
/// Returns `XX001` when the payload does not expand or decode as the field's
/// declared type; resource and cancellation errors pass through.
pub(crate) fn decode_into_lane(
    lane: &mut LaneBuilder,
    relation: &str,
    id: &str,
    field: &RowFieldMeta,
    raw: &[u8],
    hooks: &StorageValueHooks,
) -> Result<(), CassieError> {
    let expanded = match expand_column_field(raw) {
        Ok(expanded) => expanded,
        Err(CassieError::Parse(detail)) => {
            return Err(corrupt_row(
                relation,
                id,
                &format!("field '{}' does not expand: {detail}", field.name),
            ))
        }
        Err(other) => return Err(other),
    };
    let cell = decode_compact_scalar(&field.data_type, &expanded, hooks.decode_json)
        .map_err(CassieError::from)
        .map_err(|error| match error {
            CassieError::Parse(detail) => corrupt_row(
                relation,
                id,
                &format!("field '{}' does not decode: {detail}", field.name),
            ),
            other => other,
        })?;
    lane.push_cell(cell, &field.data_type, hooks)
        .map_err(|error| match error {
            CassieError::Parse(detail) => corrupt_row(
                relation,
                id,
                &format!("field '{}' does not decode: {detail}", field.name),
            ),
            other => other,
        })
}
