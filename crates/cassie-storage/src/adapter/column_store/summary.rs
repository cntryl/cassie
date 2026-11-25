//! Rebuildable per-segment pruning summaries of column-authoritative tables
//! (#1357).
//!
//! A summary record covers one contiguous row-id range of a table (a segment of
//! up to [`SEGMENT_ROWS`] rows when built). It stores the exact fingerprint of
//! the live markers it was built from and, per summarizable field, the row
//! count, non-NULL count, minimum and maximum. Records are derived metadata:
//! the field keys stay the only source of truth, and a record is trusted only
//! while its marker fingerprint, schema version, format version and checksum
//! all match what a reader sees now.

use cassie_core::types::semantic::compare_values;
use cassie_core::types::{DataType, Value};
use sha2::{Digest, Sha256};

/// Rows per segment when summaries are built.
pub(crate) const SEGMENT_ROWS: usize = 256;
/// A range that grows beyond this many rows is cut into [`SEGMENT_ROWS`] pieces.
pub(crate) const SPLIT_ROWS: usize = SEGMENT_ROWS * 2;
/// Longest text bound a record stores; a longer value leaves the field unsummarized.
pub(crate) const MAX_TEXT_BOUND_BYTES: usize = 256;

const MAGIC: &[u8; 4] = b"CAS1";
/// Encoded record format. A record of any other version is bypassed.
pub(crate) const FORMAT_VERSION: u16 = 1;
const CHECKSUM_BYTES: usize = 32;

/// The scalar kinds a record can bound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SummaryKind {
    Integer,
    Text,
    Boolean,
}

impl SummaryKind {
    /// The kind a declared column type is summarized as, if any. Types whose
    /// SQL comparison is not plain scalar ordering (floats, padded characters,
    /// temporal, composite, domain) are never summarized.
    pub(crate) fn of(data_type: &DataType) -> Option<Self> {
        match data_type {
            DataType::SmallInt | DataType::Int | DataType::BigInt => Some(Self::Integer),
            DataType::Text | DataType::Varchar { .. } => Some(Self::Text),
            DataType::Boolean => Some(Self::Boolean),
            _ => None,
        }
    }

    fn tag(self) -> u8 {
        match self {
            Self::Integer => 1,
            Self::Text => 2,
            Self::Boolean => 3,
        }
    }

    fn from_tag(tag: u8) -> Option<Self> {
        match tag {
            1 => Some(Self::Integer),
            2 => Some(Self::Text),
            3 => Some(Self::Boolean),
            _ => None,
        }
    }

    /// The summary scalar a decoded stored value contributes, or `None` when
    /// the value cannot be bounded (the field is then left unsummarized).
    pub(crate) fn scalar(self, value: &serde_json::Value) -> Option<Value> {
        match self {
            Self::Integer => value.as_i64().map(Value::Int64),
            Self::Text => value
                .as_str()
                .filter(|text| text.len() <= MAX_TEXT_BOUND_BYTES)
                .map(|text| Value::String(text.to_owned())),
            Self::Boolean => value.as_bool().map(Value::Bool),
        }
    }

    /// A SQL literal as a scalar of this kind, or `None` when the literal's
    /// type is not directly comparable (the predicate then prunes nothing).
    pub(crate) fn literal(self, literal: &serde_json::Value) -> Option<Value> {
        match (self, literal) {
            (Self::Integer, serde_json::Value::Number(number)) => number
                .as_i64()
                .map(Value::Int64)
                .or_else(|| number.as_f64().map(Value::Float64)),
            (Self::Text, serde_json::Value::String(text)) => Some(Value::String(text.clone())),
            (Self::Boolean, serde_json::Value::Bool(flag)) => Some(Value::Bool(*flag)),
            _ => None,
        }
    }
}

/// Bounds of one field within one segment.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FieldSummary {
    pub field_id: u32,
    pub kind: SummaryKind,
    pub non_null: u32,
    pub min: Option<Value>,
    pub max: Option<Value>,
}

/// One segment record.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SummaryRecord {
    /// Row id the segment starts at; empty for the first segment of a table.
    pub start: String,
    pub schema_version: u32,
    pub row_count: u32,
    pub fingerprint: [u8; 32],
    pub fields: Vec<FieldSummary>,
}

/// Incremental fingerprint of a run of live markers.
pub(crate) struct MarkerFingerprint {
    hasher: Sha256,
    count: u32,
}

impl MarkerFingerprint {
    pub(crate) fn new() -> Self {
        Self {
            hasher: Sha256::new(),
            count: 0,
        }
    }

    /// Folds one live marker (its full key and value) into the fingerprint.
    pub(crate) fn add(&mut self, key: &[u8], marker: &[u8]) {
        self.hasher
            .update(u32::try_from(key.len()).unwrap_or(u32::MAX).to_be_bytes());
        self.hasher.update(key);
        self.hasher.update(
            u32::try_from(marker.len())
                .unwrap_or(u32::MAX)
                .to_be_bytes(),
        );
        self.hasher.update(marker);
        self.count = self.count.saturating_add(1);
    }

    pub(crate) fn finish(self) -> (u32, [u8; 32]) {
        (self.count, self.hasher.finalize().into())
    }
}

impl SummaryRecord {
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&FORMAT_VERSION.to_be_bytes());
        out.extend_from_slice(&self.schema_version.to_be_bytes());
        out.extend_from_slice(&self.row_count.to_be_bytes());
        out.extend_from_slice(&self.fingerprint);
        write_bytes(&mut out, self.start.as_bytes());
        out.extend_from_slice(
            &u16::try_from(self.fields.len())
                .unwrap_or(u16::MAX)
                .to_be_bytes(),
        );
        for field in &self.fields {
            out.extend_from_slice(&field.field_id.to_be_bytes());
            out.push(field.kind.tag());
            out.extend_from_slice(&field.non_null.to_be_bytes());
            write_scalar(&mut out, field.min.as_ref());
            write_scalar(&mut out, field.max.as_ref());
        }
        let checksum = Sha256::digest(&out);
        out.extend_from_slice(&checksum);
        out
    }

    /// Decodes and validates a stored record: checksum, format version and
    /// internal consistency. Any defect is `None`; the caller bypasses it.
    pub(crate) fn decode(bytes: &[u8]) -> Option<Self> {
        let split = bytes.len().checked_sub(CHECKSUM_BYTES)?;
        let (payload, checksum) = bytes.split_at(split);
        if Sha256::digest(payload).as_slice() != checksum {
            return None;
        }
        let mut reader = Reader { bytes: payload };
        if reader.take(4)? != MAGIC || reader.u16()? != FORMAT_VERSION {
            return None;
        }
        let schema_version = reader.u32()?;
        let row_count = reader.u32()?;
        let fingerprint: [u8; 32] = reader.take(32)?.try_into().ok()?;
        let start = String::from_utf8(reader.bytes()?.to_vec()).ok()?;
        let field_count = usize::from(reader.u16()?);
        let mut fields = Vec::with_capacity(field_count);
        for _ in 0..field_count {
            let field_id = reader.u32()?;
            let kind = SummaryKind::from_tag(reader.u8()?)?;
            let non_null = reader.u32()?;
            let min = reader.scalar(kind).ok()?;
            let max = reader.scalar(kind).ok()?;
            if non_null > row_count || (non_null == 0) != (min.is_none() && max.is_none()) {
                return None;
            }
            fields.push(FieldSummary {
                field_id,
                kind,
                non_null,
                min,
                max,
            });
        }
        reader.bytes.is_empty().then_some(Self {
            start,
            schema_version,
            row_count,
            fingerprint,
            fields,
        })
    }
}

fn write_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&u32::try_from(bytes.len()).unwrap_or(u32::MAX).to_be_bytes());
    out.extend_from_slice(bytes);
}

fn write_scalar(out: &mut Vec<u8>, value: Option<&Value>) {
    match value {
        Some(Value::Int64(value)) => {
            out.push(1);
            out.extend_from_slice(&value.to_be_bytes());
        }
        Some(Value::String(value)) => {
            out.push(1);
            write_bytes(out, value.as_bytes());
        }
        Some(Value::Bool(value)) => {
            out.push(1);
            out.push(u8::from(*value));
        }
        _ => out.push(0),
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        let (head, tail) = self.bytes.split_at_checked(len)?;
        self.bytes = tail;
        Some(head)
    }

    fn u8(&mut self) -> Option<u8> {
        self.take(1)?.first().copied()
    }

    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_be_bytes(self.take(2)?.try_into().ok()?))
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }

    fn bytes(&mut self) -> Option<&'a [u8]> {
        let len = usize::try_from(self.u32()?).ok()?;
        self.take(len)
    }

    /// `Ok(None)` is an absent bound, `Err(())` a malformed one.
    fn scalar(&mut self, kind: SummaryKind) -> Result<Option<Value>, ()> {
        match self.u8().ok_or(())? {
            0 => Ok(None),
            1 => self.present_scalar(kind).map(Some).ok_or(()),
            _ => Err(()),
        }
    }

    fn present_scalar(&mut self, kind: SummaryKind) -> Option<Value> {
        Some(match kind {
            SummaryKind::Integer => {
                Value::Int64(i64::from_be_bytes(self.take(8)?.try_into().ok()?))
            }
            SummaryKind::Text => {
                let text = String::from_utf8(self.bytes()?.to_vec()).ok()?;
                (text.len() <= MAX_TEXT_BOUND_BYTES).then_some(())?;
                Value::String(text)
            }
            SummaryKind::Boolean => match self.u8()? {
                0 => Value::Bool(false),
                1 => Value::Bool(true),
                _ => return None,
            },
        })
    }
}

/// A comparison a conjunctive scan filter applies to one field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PruneOp {
    Eq,
    Lt,
    Lte,
    Gt,
    Gte,
    IsNull,
    IsNotNull,
}

/// One filter conjunct resolved against the table's schema.
#[derive(Clone, Debug)]
pub(crate) struct PrunePredicate {
    pub field_id: u32,
    pub kind: SummaryKind,
    pub op: PruneOp,
    pub literal: Option<Value>,
}

impl PrunePredicate {
    /// Whether no row of the segment can satisfy this conjunct, judged only
    /// from the record's bounds. `false` means "cannot rule the segment out".
    fn excludes(&self, record: &SummaryRecord) -> bool {
        let Some(field) = record
            .fields
            .iter()
            .find(|field| field.field_id == self.field_id && field.kind == self.kind)
        else {
            return false;
        };
        let null_count = record.row_count.saturating_sub(field.non_null);
        match self.op {
            PruneOp::IsNull => null_count == 0,
            PruneOp::IsNotNull => field.non_null == 0,
            _ if field.non_null == 0 => true,
            op => {
                let (Some(min), Some(max), Some(literal)) = (
                    field.min.as_ref(),
                    field.max.as_ref(),
                    self.literal.as_ref(),
                ) else {
                    return false;
                };
                let below_min = compare_values(literal, min).is_lt();
                let above_max = compare_values(literal, max).is_gt();
                match op {
                    PruneOp::Eq => below_min || above_max,
                    PruneOp::Lt => !compare_values(min, literal).is_lt(),
                    PruneOp::Lte => compare_values(min, literal).is_gt(),
                    PruneOp::Gt => !compare_values(max, literal).is_gt(),
                    PruneOp::Gte => compare_values(max, literal).is_lt(),
                    PruneOp::IsNull | PruneOp::IsNotNull => false,
                }
            }
        }
    }
}

/// Whether the record proves that no row of its segment satisfies the
/// conjunction `predicates`.
pub(crate) fn segment_excluded(record: &SummaryRecord, predicates: &[PrunePredicate]) -> bool {
    predicates
        .iter()
        .any(|predicate| predicate.excludes(record))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(non_null: u32, min: i64, max: i64) -> SummaryRecord {
        SummaryRecord {
            start: String::new(),
            schema_version: 3,
            row_count: 10,
            fingerprint: [7; 32],
            fields: vec![FieldSummary {
                field_id: 1,
                kind: SummaryKind::Integer,
                non_null,
                min: (non_null > 0).then_some(Value::Int64(min)),
                max: (non_null > 0).then_some(Value::Int64(max)),
            }],
        }
    }

    fn predicate(op: PruneOp, literal: Option<Value>) -> PrunePredicate {
        PrunePredicate {
            field_id: 1,
            kind: SummaryKind::Integer,
            op,
            literal,
        }
    }

    #[test]
    fn should_round_trip_a_record_rejecting_any_flipped_byte() {
        // Arrange
        let record = record(8, -5, 40);
        let encoded = record.encode();

        // Act
        let decoded = SummaryRecord::decode(&encoded);
        let rejected = (0..encoded.len()).all(|index| {
            let mut changed = encoded.clone();
            changed[index] ^= 0x01;
            SummaryRecord::decode(&changed).is_none()
        });

        // Assert
        assert_eq!(decoded, Some(record));
        assert!(rejected);
    }

    #[test]
    fn should_exclude_segments_only_when_bounds_prove_no_match() {
        // Arrange
        let segment = record(8, 10, 20);
        let cases = [
            (PruneOp::Eq, 9, true),
            (PruneOp::Eq, 10, false),
            (PruneOp::Eq, 21, true),
            (PruneOp::Lt, 10, true),
            (PruneOp::Lt, 11, false),
            (PruneOp::Lte, 9, true),
            (PruneOp::Lte, 10, false),
            (PruneOp::Gt, 20, true),
            (PruneOp::Gt, 19, false),
            (PruneOp::Gte, 21, true),
            (PruneOp::Gte, 20, false),
        ];

        // Act
        // Assert
        for (op, literal, excluded) in cases {
            assert_eq!(
                segment_excluded(&segment, &[predicate(op, Some(Value::Int64(literal)))]),
                excluded,
                "{op:?} {literal}"
            );
        }
    }

    #[test]
    fn should_use_null_counts_for_null_tests() {
        // Arrange
        let full = record(10, 1, 2);
        let partial = record(4, 1, 2);
        let empty = record(0, 0, 0);

        // Act
        // Assert
        assert!(segment_excluded(&full, &[predicate(PruneOp::IsNull, None)]));
        assert!(!segment_excluded(
            &partial,
            &[predicate(PruneOp::IsNull, None)]
        ));
        assert!(segment_excluded(
            &empty,
            &[predicate(PruneOp::IsNotNull, None)]
        ));
        assert!(segment_excluded(
            &empty,
            &[predicate(PruneOp::Eq, Some(Value::Int64(1)))]
        ));
    }

    #[test]
    fn should_compare_integer_bounds_with_fractional_literals() {
        // Arrange
        let segment = record(8, 10, 20);

        // Act
        // Assert
        assert!(segment_excluded(
            &segment,
            &[predicate(PruneOp::Gt, Some(Value::Float64(20.5)))]
        ));
        assert!(!segment_excluded(
            &segment,
            &[predicate(PruneOp::Gt, Some(Value::Float64(19.5)))]
        ));
        assert!(!segment_excluded(
            &segment,
            &[predicate(PruneOp::Eq, Some(Value::Float64(10.5)))]
        ));
    }
}
