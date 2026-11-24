//! Id-ordered merge of one marker batch with the selected field iterators.
//!
//! Markers and field keys of a column table are sorted by row id within their
//! own key ranges. One range iterator per selected field is walked alongside
//! the batch's markers:
//!
//! * a field entry whose id sorts before the marker is an orphan: `XX001` for a
//!   complete-source scan, skipped for a visited-row scan (D6);
//! * an equal id carries the value;
//! * a greater id, or an exhausted iterator, means the row has no stored value:
//!   `XX001` when the field is required at the marker's schema version, SQL
//!   NULL otherwise;
//! * entries left after the last marker are orphans.

use super::decode::{decode_into_lane, LaneBuilder};
use super::{check_controls, Marker};
use crate::app::CassieError;
use crate::midge::adapter::column_store::validation::{
    corrupt_row, field_is_required, missing_field,
};
use crate::midge::adapter::{Midge, RowTraversal};
use crate::midge::row_blob::RowFieldMeta;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use bytes::Bytes;
use std::cmp::Ordering;

/// One selected field's range iterator and its next undecided entry.
pub(super) struct FieldStream<'tx> {
    iter: cntryl_midge::ScanIterator<'tx>,
    prefix: Vec<u8>,
    peeked: Option<(Bytes, Bytes)>,
}

impl<'tx> FieldStream<'tx> {
    pub(super) fn new(iter: cntryl_midge::ScanIterator<'tx>, prefix: Vec<u8>) -> Self {
        Self {
            iter,
            prefix,
            peeked: None,
        }
    }

    fn fill(&mut self) -> Result<(), CassieError> {
        if self.peeked.is_none() {
            self.peeked = self.iter.next().transpose().map_err(CassieError::from)?;
        }
        Ok(())
    }

    /// The row id bytes of the pending entry.
    fn pending_id(&self) -> Option<&[u8]> {
        let (key, _) = self.peeked.as_ref()?;
        // A key outside the field prefix cannot occur in a prefix-bounded
        // range; treat it as no entry so the merge never misorders ids.
        key.strip_prefix(self.prefix.as_slice())
    }
}

pub(super) struct MergeInput<'a, 'tx> {
    pub midge: &'a Midge,
    pub relation: &'a str,
    pub fields: &'a [RowFieldMeta],
    pub markers: &'a [Marker],
    pub traversal: RowTraversal,
    pub controls: &'a QueryExecutionControls,
    pub streams: &'a mut [FieldStream<'tx>],
    pub builders: &'a mut [LaneBuilder],
    pub memory: &'a mut QueryMemoryReservation,
}

fn orphan(relation: &str, id: &[u8], field: &RowFieldMeta) -> CassieError {
    corrupt_row(
        relation,
        &String::from_utf8_lossy(id),
        &format!(
            "field '{}' has a value but the row has no live marker",
            field.name
        ),
    )
}

/// Walks the batch's markers in id order and fills one lane per selected field.
///
/// # Errors
///
/// Returns `XX001` for a missing required value, an undecodable value, or an
/// orphan on a complete-source scan; budget and cancellation errors pass through.
pub(super) fn merge_batch(input: MergeInput<'_, '_>) -> Result<(), CassieError> {
    let MergeInput {
        midge,
        relation,
        fields,
        markers,
        traversal,
        controls,
        streams,
        builders,
        memory,
    } = input;
    let strict = traversal == RowTraversal::CompleteSource;
    for marker in markers {
        check_controls(controls)?;
        for ((field, stream), builder) in fields
            .iter()
            .zip(streams.iter_mut())
            .zip(builders.iter_mut())
        {
            loop {
                stream.fill()?;
                let order = stream.pending_id().map(|id| id.cmp(marker.id.as_bytes()));
                match order {
                    Some(Ordering::Less) => {
                        if strict {
                            let id = stream.pending_id().unwrap_or_default();
                            return Err(orphan(relation, id, field));
                        }
                        stream.peeked = None;
                    }
                    Some(Ordering::Equal) => {
                        let Some((_, raw)) = stream.peeked.take() else {
                            break;
                        };
                        let context = ValueContext {
                            midge,
                            relation,
                            controls,
                        };
                        take_value(&context, (marker, field), &raw, builder, memory)?;
                        break;
                    }
                    Some(Ordering::Greater) | None => {
                        if field_is_required(field, marker.version) {
                            return Err(missing_field(relation, &marker.id, field));
                        }
                        builder.push_null();
                        break;
                    }
                }
            }
        }
    }
    if strict {
        for (field, stream) in fields.iter().zip(streams.iter_mut()) {
            check_controls(controls)?;
            stream.fill()?;
            if let Some(id) = stream.pending_id() {
                return Err(orphan(relation, id, field));
            }
        }
    }
    Ok(())
}

struct ValueContext<'a> {
    midge: &'a Midge,
    relation: &'a str,
    controls: &'a QueryExecutionControls,
}

fn take_value(
    context: &ValueContext<'_>,
    (marker, field): (&Marker, &RowFieldMeta),
    raw: &Bytes,
    builder: &mut LaneBuilder,
    memory: &mut QueryMemoryReservation,
) -> Result<(), CassieError> {
    let ValueContext {
        midge,
        relation,
        controls,
    } = *context;
    let counters = midge.column_field_reads();
    let vector = midge.column_vector_counters();
    counters.record_payload(raw.len());
    vector.record_value_fetched();
    check_controls(controls)?;
    let logical_len = crate::midge::adapter::column_store::column_payload_logical_len(raw);
    let field_bytes = if matches!(
        field.data_type,
        crate::types::DataType::Json | crate::types::DataType::Jsonb
    ) {
        crate::runtime::accounted::json::serialized_decode_bytes(logical_len)?
    } else {
        logical_len
            .checked_mul(
                crate::midge::adapter::streaming_scans::decode_expansion_factor(&field.data_type),
            )
            .ok_or_else(|| {
                CassieError::ResourceLimit("column vector field accounting overflow".to_owned())
            })?
    };
    memory.try_grow(field_bytes)?;
    counters.record_decode();
    vector.record_value_decoded();
    decode_into_lane(builder, relation, &marker.id, field, raw)?;
    counters.record_retained(field_bytes);
    check_controls(controls)
}
