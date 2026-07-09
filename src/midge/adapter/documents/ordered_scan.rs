use std::collections::HashSet;
use std::time::{Duration, Instant};

use crate::midge::adapter::OrderedColumnStoreScanRequest;

use super::super::OrderedRowBound;
use super::{
    decode_projected_row, decode_projected_row_with_aliases, decode_row, key_encoding, CassieError,
    DocumentRef, Midge, MidgeScanTimings, Query, RowDecode, RowSchema,
};

fn empty_scan_result(started: Instant) -> (Vec<Vec<DocumentRef>>, MidgeScanTimings) {
    (
        Vec::new(),
        MidgeScanTimings {
            scan: started.elapsed(),
            row_decode: Duration::ZERO,
        },
    )
}

fn ordered_scan_timings(started: Instant, row_decode: Duration) -> MidgeScanTimings {
    MidgeScanTimings {
        scan: started.elapsed().saturating_sub(row_decode),
        row_decode,
    }
}

fn decode_ordered_scan_entry(
    config: &OrderedRowScanConfig<'_>,
    selected: OrderedScanEntry,
) -> Result<DocumentRef, CassieError> {
    let payload = match config.projection {
        Some(projection) if config.include_historical_aliases => {
            decode_projected_row_with_aliases(config.row_schema, &selected.raw_value, projection)?
        }
        Some(projection) => {
            decode_projected_row(config.row_schema, &selected.raw_value, projection)?
        }
        None => decode_row(config.row_schema, &selected.raw_value)?,
    };
    Ok(DocumentRef {
        id: selected.id,
        payload,
    })
}

#[derive(Debug, Clone)]
struct OrderedScanEntry {
    id: String,
    raw_value: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OrderedScanSelection {
    Row,
    Doc,
    Both,
}

struct OrderedScanSources {
    row_prefix: Vec<u8>,
    doc_prefix: Vec<u8>,
    row_iter: cntryl_midge::ScanIterator,
    doc_iter: cntryl_midge::ScanIterator,
    row_next: Option<OrderedScanEntry>,
    doc_next: Option<OrderedScanEntry>,
}

struct OrderedRowScanConfig<'a> {
    row_schema: &'a RowSchema,
    projection: Option<&'a HashSet<String>>,
    include_historical_aliases: bool,
    batch_size: usize,
    limit: usize,
    reverse: bool,
    scan_started: Instant,
}

pub(crate) struct OrderedRowScanRequest<'a> {
    pub collection: &'a str,
    pub batch_size: usize,
    pub decode: RowDecode,
    pub start_bound: Option<&'a OrderedRowBound>,
    pub end_bound: Option<&'a OrderedRowBound>,
    pub reverse: bool,
    pub limit: Option<usize>,
}

impl OrderedScanSources {
    fn next_entry(&mut self, reverse: bool) -> Option<OrderedScanEntry> {
        let selection =
            Midge::ordered_selection(self.row_next.as_ref(), self.doc_next.as_ref(), reverse)?;
        let selected = match selection {
            OrderedScanSelection::Row => self
                .row_next
                .take()
                .expect("row entry should exist for row selection"),
            OrderedScanSelection::Doc => self
                .doc_next
                .take()
                .expect("doc entry should exist for doc selection"),
            OrderedScanSelection::Both => self
                .row_next
                .take()
                .expect("row entry should exist for duplicate selection"),
        };
        match selection {
            OrderedScanSelection::Row => {
                self.row_next = Midge::ordered_next_entry(&mut self.row_iter, &self.row_prefix);
            }
            OrderedScanSelection::Doc => {
                self.doc_next = Midge::ordered_next_entry(&mut self.doc_iter, &self.doc_prefix);
            }
            OrderedScanSelection::Both => {
                self.row_next = Midge::ordered_next_entry(&mut self.row_iter, &self.row_prefix);
                self.doc_next = Midge::ordered_next_entry(&mut self.doc_iter, &self.doc_prefix);
            }
        }
        Some(selected)
    }
}

impl Midge {
    pub(crate) fn scan_ordered_rows_batched_by_id_limit_with_timings(
        &self,
        request: OrderedRowScanRequest<'_>,
    ) -> Result<(Vec<Vec<DocumentRef>>, MidgeScanTimings), CassieError> {
        self.scan_ordered_rows_batched_by_id(request)
    }

    fn scan_ordered_rows_batched_by_id(
        &self,
        request: OrderedRowScanRequest<'_>,
    ) -> Result<(Vec<Vec<DocumentRef>>, MidgeScanTimings), CassieError> {
        let scan_started = Instant::now();
        let row_schema = self.row_schema(request.collection)?;
        let (projection, include_historical_aliases) = request.decode.into_projection();
        let tx = self.begin_data_readonly_tx()?;
        let batch_size = request.batch_size.max(1);
        let limit = request.limit.unwrap_or(usize::MAX);
        if self.collection_uses_column_store(request.collection)? {
            return Self::scan_ordered_column_store_rows_batched_by_id(
                &tx,
                OrderedColumnStoreScanRequest {
                    collection: request.collection,
                    row_schema: &row_schema,
                    batch_size,
                    projection: projection.as_ref(),
                    start_bound: request.start_bound,
                    end_bound: request.end_bound,
                    reverse: request.reverse,
                    limit,
                },
            );
        }
        if limit == 0 {
            return Ok(empty_scan_result(scan_started));
        }

        let mut sources = Self::ordered_scan_sources(
            &tx,
            request.collection,
            request.start_bound,
            request.end_bound,
            request.reverse,
        )?;
        let config = OrderedRowScanConfig {
            row_schema: &row_schema,
            projection: projection.as_ref(),
            include_historical_aliases,
            batch_size,
            limit,
            reverse: request.reverse,
            scan_started,
        };
        Self::collect_ordered_rows(&mut sources, &config)
    }

    fn ordered_scan_sources(
        tx: &cntryl_midge::Transaction,
        collection: &str,
        start_bound: Option<&OrderedRowBound>,
        end_bound: Option<&OrderedRowBound>,
        reverse: bool,
    ) -> Result<OrderedScanSources, CassieError> {
        let row_prefix = Self::row_prefix(collection);
        let doc_prefix = Self::doc_prefix(collection);
        let mut row_iter = tx
            .scan(&Self::ordered_row_query(
                &row_prefix,
                start_bound,
                end_bound,
                reverse,
            ))
            .map_err(CassieError::from)?;
        let mut doc_iter = tx
            .scan(&Self::ordered_row_query(
                &doc_prefix,
                start_bound,
                end_bound,
                reverse,
            ))
            .map_err(CassieError::from)?;
        let row_next = Self::ordered_next_entry(&mut row_iter, &row_prefix);
        let doc_next = Self::ordered_next_entry(&mut doc_iter, &doc_prefix);
        Ok(OrderedScanSources {
            row_prefix,
            doc_prefix,
            row_iter,
            doc_iter,
            row_next,
            doc_next,
        })
    }

    fn collect_ordered_rows(
        sources: &mut OrderedScanSources,
        config: &OrderedRowScanConfig<'_>,
    ) -> Result<(Vec<Vec<DocumentRef>>, MidgeScanTimings), CassieError> {
        let mut results = Vec::new();
        let mut current = Vec::with_capacity(config.batch_size);
        let mut emitted = 0usize;
        let mut row_decode = Duration::ZERO;
        while emitted < config.limit {
            let Some(selected) = sources.next_entry(config.reverse) else {
                break;
            };
            let decode_started = Instant::now();
            current.push(decode_ordered_scan_entry(config, selected)?);
            row_decode += decode_started.elapsed();
            emitted += 1;
            if current.len() >= config.batch_size {
                results.push(current);
                current = Vec::with_capacity(config.batch_size);
            }
        }
        if !current.is_empty() {
            results.push(current);
        }
        Ok((
            results,
            ordered_scan_timings(config.scan_started, row_decode),
        ))
    }

    fn ordered_row_query(
        prefix: &[u8],
        start_bound: Option<&OrderedRowBound>,
        end_bound: Option<&OrderedRowBound>,
        reverse: bool,
    ) -> Query {
        let mut query = Query::new().prefix(prefix.to_vec().into());
        if let Some(bound) = start_bound {
            query =
                query.start_key(Self::ordered_start_key(prefix, &bound.id, bound.inclusive).into());
        }
        if let Some(bound) = end_bound {
            query = query.end_key(Self::ordered_end_key(prefix, &bound.id, bound.inclusive).into());
        }
        if reverse {
            query = query.reverse();
        }
        query
    }

    fn ordered_start_key(prefix: &[u8], id: &str, inclusive: bool) -> Vec<u8> {
        let mut key = prefix.to_vec();
        key.extend_from_slice(id.as_bytes());
        if !inclusive {
            key.push(0);
        }
        key
    }

    fn ordered_end_key(prefix: &[u8], id: &str, inclusive: bool) -> Vec<u8> {
        let mut key = prefix.to_vec();
        key.extend_from_slice(id.as_bytes());
        if inclusive {
            key.push(0);
        }
        key
    }

    fn ordered_next_entry(
        iter: &mut cntryl_midge::ScanIterator,
        prefix: &[u8],
    ) -> Option<OrderedScanEntry> {
        for (raw_key, raw_value) in iter.by_ref() {
            let Some(id) = key_encoding::utf8_suffix_after_prefix(&raw_key, prefix) else {
                continue;
            };
            if id.is_empty() {
                continue;
            }
            return Some(OrderedScanEntry { id, raw_value });
        }

        None
    }

    fn ordered_selection(
        row: Option<&OrderedScanEntry>,
        doc: Option<&OrderedScanEntry>,
        reverse: bool,
    ) -> Option<OrderedScanSelection> {
        match (row, doc) {
            (Some(_), None) => Some(OrderedScanSelection::Row),
            (None, Some(_)) => Some(OrderedScanSelection::Doc),
            (Some(row), Some(doc)) => match row.id.cmp(&doc.id) {
                std::cmp::Ordering::Less => {
                    if reverse {
                        Some(OrderedScanSelection::Doc)
                    } else {
                        Some(OrderedScanSelection::Row)
                    }
                }
                std::cmp::Ordering::Greater => {
                    if reverse {
                        Some(OrderedScanSelection::Row)
                    } else {
                        Some(OrderedScanSelection::Doc)
                    }
                }
                std::cmp::Ordering::Equal => Some(OrderedScanSelection::Both),
            },
            (None, None) => None,
        }
    }
}
