use std::collections::HashSet;
use std::mem::size_of;

use bytes::Bytes;

use crate::midge::adapter::streaming_scans::provisional_controlled_document_bytes;
use crate::midge::adapter::AccountedDocument;
use crate::midge::row_blob::{RowFieldMeta, RowSchema};
use crate::runtime::accounted::Accounted;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::DataType;

use super::{
    decode_projected_row, decode_projected_row_with_aliases, decode_row, CassieError, DocumentRef,
    Midge, OrderedRowBound, OrderedRowScanRequest, Query,
};

#[cfg(test)]
#[path = "controlled/tests.rs"]
mod tests;

pub(crate) struct ControlledOrderedRowCursor {
    tx: cntryl_midge::Transaction,
    row_schema: RowSchema,
    projection: Option<HashSet<String>>,
    include_historical_aliases: bool,
    row: OrderedSource,
    doc: OrderedSource,
    reverse: bool,
    remaining: usize,
    _schema_memory: QueryMemoryReservation,
    _descriptor_memory: QueryMemoryReservation,
}

struct OrderedSource {
    prefix: Vec<u8>,
    lower: Option<Vec<u8>>,
    upper: Option<Vec<u8>>,
    last_key: Option<Accounted<Bytes>>,
    next: Option<AccountedRawEntry>,
    exhausted: bool,
}

struct AccountedRawEntry {
    id: String,
    raw: Bytes,
    _memory: QueryMemoryReservation,
}

impl Midge {
    pub(crate) fn open_controlled_ordered_row_cursor(
        &self,
        request: OrderedRowScanRequest<'_>,
        controls: &QueryExecutionControls,
    ) -> Result<ControlledOrderedRowCursor, CassieError> {
        check_controls(controls)?;
        // Loading catalog/schema descriptors remains the documented planning scope. The
        // retained execution clone is separately reserved before it is constructed.
        let collection = self.canonical_collection_name(request.collection);
        let descriptor = self.row_schema(&collection)?;
        let schema_memory = controls.reserve_query_memory(row_schema_bytes(&descriptor)?)?;
        let row_schema = descriptor.clone();
        drop(descriptor);
        let projection_bytes = match &request.decode {
            crate::midge::adapter::RowDecode::Full => 0,
            crate::midge::adapter::RowDecode::Projected(fields)
            | crate::midge::adapter::RowDecode::ProjectedHistorical(fields) => {
                fields.iter().try_fold(0usize, |bytes, field| {
                    add(
                        bytes,
                        add(field.len().checked_mul(3).ok_or_else(overflow)?, 256)?,
                    )
                })?
            }
        };
        let prefix_bytes = add(collection.len(), 32)?;
        let bound_bytes = [request.start_bound, request.end_bound]
            .into_iter()
            .flatten()
            .try_fold(0usize, |bytes, bound| {
                add(
                    bytes,
                    add(prefix_bytes, add(bound.id.len(), 1)?)?
                        .checked_mul(4)
                        .ok_or_else(overflow)?,
                )
            })?;
        let descriptor_bytes = add(
            size_of::<ControlledOrderedRowCursor>(),
            add(
                prefix_bytes.checked_mul(4).ok_or_else(overflow)?,
                add(bound_bytes, projection_bytes)?,
            )?,
        )?;
        let descriptor_memory = controls.reserve_query_memory(descriptor_bytes)?;
        let (projection, include_historical_aliases) = request.decode.into_projection();
        let row = OrderedSource::new(
            Self::row_prefix(row_schema.relation_id),
            request.start_bound,
            request.end_bound,
        );
        let doc = OrderedSource::new(
            Self::doc_prefix(&collection),
            request.start_bound,
            request.end_bound,
        );
        let tx = self.begin_data_readonly_tx_for(&collection)?;
        check_controls(controls)?;
        Ok(ControlledOrderedRowCursor {
            tx,
            row_schema,
            projection,
            include_historical_aliases,
            row,
            doc,
            reverse: request.reverse,
            remaining: request.limit.unwrap_or(usize::MAX),
            _schema_memory: schema_memory,
            _descriptor_memory: descriptor_memory,
        })
    }
}

impl ControlledOrderedRowCursor {
    pub(crate) fn next_accounted_document(
        &mut self,
        midge: &Midge,
        controls: &QueryExecutionControls,
    ) -> Result<Option<AccountedDocument>, CassieError> {
        check_controls(controls)?;
        if self.remaining == 0 {
            return Ok(None);
        }
        self.row.fill(&self.tx, midge, self.reverse, controls)?;
        self.doc.fill(&self.tx, midge, self.reverse, controls)?;
        let selected = match (&self.row.next, &self.doc.next) {
            (None, None) => return Ok(None),
            (Some(_), None) => self.row.next.take(),
            (None, Some(_)) => self.doc.next.take(),
            (Some(row), Some(doc)) => match row.id.cmp(&doc.id) {
                std::cmp::Ordering::Equal => {
                    self.doc.next.take();
                    self.row.next.take()
                }
                std::cmp::Ordering::Less if !self.reverse => self.row.next.take(),
                std::cmp::Ordering::Greater if self.reverse => self.row.next.take(),
                _ => self.doc.next.take(),
            },
        }
        .expect("a selected ordered source has a lookahead row");
        let retained = provisional_controlled_document_bytes(
            &self.row_schema,
            self.projection.as_ref(),
            self.include_historical_aliases,
            selected.id.len(),
            selected.raw.len(),
        )?;
        let AccountedRawEntry { id, raw, _memory } = selected;
        let document =
            AccountedDocument::try_build_fresh_decode_optional(controls, retained, || {
                let payload = match self.projection.as_ref() {
                    Some(fields) if self.include_historical_aliases => {
                        decode_projected_row_with_aliases(&self.row_schema, &raw, fields)?
                    }
                    Some(fields) => decode_projected_row(&self.row_schema, &raw, fields)?,
                    None => decode_row(&self.row_schema, &raw)?,
                };
                Ok(Some(DocumentRef { id, payload }))
            })?;
        self.remaining -= 1;
        check_controls(controls)?;
        Ok(document)
    }
}

impl OrderedSource {
    fn new(
        prefix: Vec<u8>,
        start: Option<&OrderedRowBound>,
        end: Option<&OrderedRowBound>,
    ) -> Self {
        let lower =
            start.map(|bound| Midge::ordered_start_key(&prefix, &bound.id, bound.inclusive));
        let upper = end.map(|bound| Midge::ordered_end_key(&prefix, &bound.id, bound.inclusive));
        Self {
            prefix,
            lower,
            upper,
            last_key: None,
            next: None,
            exhausted: false,
        }
    }

    fn fill(
        &mut self,
        tx: &cntryl_midge::Transaction,
        midge: &Midge,
        reverse: bool,
        controls: &QueryExecutionControls,
    ) -> Result<(), CassieError> {
        while self.next.is_none() && !self.exhausted {
            check_controls(controls)?;
            let lower_len = if reverse {
                self.lower.as_ref().map_or(0, Vec::len)
            } else {
                self.last_key
                    .as_ref()
                    .map_or(self.lower.as_ref().map_or(0, Vec::len), |key| {
                        key.get().len() + 1
                    })
            };
            let upper_len = if reverse {
                self.last_key
                    .as_ref()
                    .map_or(self.upper.as_ref().map_or(0, Vec::len), |key| {
                        key.get().len()
                    })
            } else {
                self.upper.as_ref().map_or(0, Vec::len)
            };
            let _query_memory = controls
                .reserve_query_memory(add(self.prefix.len(), add(lower_len, upper_len)?)?)?;
            let mut query = Query::new().prefix(self.prefix.clone().into()).limit(1);
            if reverse {
                if let Some(lower) = &self.lower {
                    query = query.start_key(lower.clone().into());
                }
                if let Some(last) = &self.last_key {
                    query = query.end_key(last.get().clone());
                } else if let Some(upper) = &self.upper {
                    query = query.end_key(upper.clone().into());
                }
                query = query.reverse();
            } else {
                if let Some(last) = &self.last_key {
                    let mut key = Vec::with_capacity(lower_len);
                    key.extend_from_slice(last.get());
                    key.push(0);
                    query = query.start_key(key.into());
                } else if let Some(lower) = &self.lower {
                    query = query.start_key(lower.clone().into());
                }
                if let Some(upper) = &self.upper {
                    query = query.end_key(upper.clone().into());
                }
            }
            let entry = tx
                .scan(&query)
                .map_err(CassieError::from)?
                .next()
                .transpose()
                .map_err(CassieError::from)?;
            let Some((key, raw)) = entry else {
                self.exhausted = true;
                break;
            };
            check_controls(controls)?;
            midge.record_query_scan_entry();
            if crate::midge::adapter::query_scan_control::should_cancel_controlled_query_scan() {
                return Err(CassieError::QueryCancelled);
            }
            self.last_key = Some(Accounted::try_new(controls, key.len(), || key.clone())?);
            let Some(suffix) = key.strip_prefix(self.prefix.as_slice()) else {
                continue;
            };
            let Ok(id) = std::str::from_utf8(suffix) else {
                continue;
            };
            if id.is_empty() {
                continue;
            }
            let bytes = add(
                size_of::<AccountedRawEntry>(),
                add(key.len(), add(raw.len(), id.len())?)?,
            )?;
            let memory = controls.reserve_query_memory(bytes)?;
            self.next = Some(AccountedRawEntry {
                id: id.to_owned(),
                raw,
                _memory: memory,
            });
        }
        Ok(())
    }
}

fn row_schema_bytes(schema: &RowSchema) -> Result<usize, CassieError> {
    schema.fields.iter().try_fold(
        add(
            size_of::<RowSchema>(),
            schema
                .fields
                .len()
                .checked_mul(size_of::<RowFieldMeta>())
                .ok_or_else(overflow)?,
        )?,
        |bytes, field| {
            let alias_bytes = field.aliases.iter().try_fold(
                field
                    .aliases
                    .len()
                    .checked_mul(size_of::<String>())
                    .ok_or_else(overflow)?,
                |bytes, alias| add(bytes, alias.len()),
            )?;
            add(
                bytes,
                add(
                    field.name.len(),
                    add(
                        field.normalized_name.len(),
                        add(alias_bytes, data_type_heap_bytes(&field.data_type)?)?,
                    )?,
                )?,
            )
        },
    )
}

fn data_type_heap_bytes(data_type: &DataType) -> Result<usize, CassieError> {
    match data_type {
        DataType::Array(inner) => add(size_of::<DataType>(), data_type_heap_bytes(inner)?),
        _ => Ok(0),
    }
}

fn check_controls(controls: &QueryExecutionControls) -> Result<(), CassieError> {
    if controls.is_cancelled() {
        return Err(CassieError::QueryCancelled);
    }
    if controls.is_timed_out() {
        return Err(CassieError::DeadlineExceeded);
    }
    Ok(())
}

fn add(left: usize, right: usize) -> Result<usize, CassieError> {
    left.checked_add(right).ok_or_else(overflow)
}

fn overflow() -> CassieError {
    CassieError::ResourceLimit("controlled ordered-row accounting overflow".to_owned())
}
