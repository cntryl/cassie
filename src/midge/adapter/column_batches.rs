use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use super::*;

const COLUMN_BATCH_ENCODING_VERSION: u32 = 1;
const COLUMN_BATCH_CODEC_VERSION: u32 = 1;

struct EncodedColumnBatch {
    codec_name: String,
    codec_version: u32,
    uncompressed_len: usize,
    compressed_len: usize,
    checksum: String,
}

impl Midge {
    pub fn rebuild_column_batches_for_collection(
        &self,
        collection: &str,
    ) -> Result<(), CassieError> {
        for index in self.list_indexes()? {
            if index.collection == collection && index.kind == IndexKind::Column {
                self.rebuild_column_batches_for_index(&index)?;
            }
        }
        Ok(())
    }

    pub fn rebuild_column_batches_for_index(
        &self,
        index: &IndexMeta,
    ) -> Result<ColumnBatchMetadata, CassieError> {
        if index.kind != IndexKind::Column {
            return Err(CassieError::Unsupported(
                "column batch rebuild requires a column index".to_string(),
            ));
        }

        let fields = index.normalized_fields();
        let segment_size = column_index_segment_size(index)?;
        let mut documents = self.scan_documents(&index.collection)?;
        documents.sort_by(|left, right| left.id.cmp(&right.id));

        let schema_epoch = self.schema_epoch()?;
        let mut segments = Vec::new();
        let mut payloads = Vec::new();
        for (segment_id, chunk) in documents.chunks(segment_size).enumerate() {
            let segment_id = segment_id as u64;
            let rows = chunk
                .iter()
                .map(|document| ColumnBatchRow {
                    row_id: document.id.clone(),
                    values: column_values(&document.payload, fields.as_slice()),
                })
                .collect::<Vec<_>>();
            let value_count = rows.len().saturating_mul(fields.len());
            let (payload, codec) = encode_column_batch_payload(rows, fields.as_slice())?;
            segments.push(ColumnBatchSegmentMeta {
                segment_id,
                row_id_start: chunk.first().map(|document| document.id.clone()),
                row_id_end: chunk.last().map(|document| document.id.clone()),
                row_count: chunk.len(),
                null_bitmap_available: true,
                encoding_version: COLUMN_BATCH_ENCODING_VERSION,
                codec: ColumnBatchCodecMeta {
                    codec_name: codec.codec_name,
                    codec_version: codec.codec_version,
                    uncompressed_len: codec.uncompressed_len,
                    compressed_len: codec.compressed_len,
                    value_count,
                    null_bitmap_encoding: "inline-json-null".to_string(),
                    checksum: Some(codec.checksum),
                },
            });
            payloads.push((segment_id, payload));
        }

        let metadata = ColumnBatchMetadata {
            collection: index.collection.clone(),
            index_name: index.name.clone(),
            schema_epoch,
            fields,
            segment_size,
            segments,
        };

        let mut schema_tx = self.begin_schema_rw_tx()?;
        Self::delete_keys_with_prefix(
            &mut schema_tx,
            Self::column_batch_index_prefix(&index.collection, &index.name),
        )?;
        schema_tx
            .put(
                Self::column_batch_metadata_key(&index.collection, &index.name),
                serde_json::to_vec(&metadata)
                    .map_err(|error| CassieError::Parse(error.to_string()))?,
                None,
            )
            .map_err(CassieError::from)?;
        schema_tx
            .commit(WriteOptions::sync())
            .map_err(CassieError::from)?;

        let mut data_tx = self.begin_data_rw_tx()?;
        Self::delete_keys_with_prefix(
            &mut data_tx,
            Self::column_batch_index_prefix(&index.collection, &index.name),
        )?;
        for (segment_id, payload) in payloads {
            data_tx
                .put(
                    Self::column_batch_segment_key(&index.collection, &index.name, segment_id),
                    serde_json::to_vec(&payload)
                        .map_err(|error| CassieError::Parse(error.to_string()))?,
                    None,
                )
                .map_err(CassieError::from)?;
        }
        data_tx
            .commit(WriteOptions::sync())
            .map_err(CassieError::from)?;

        Ok(metadata)
    }

    pub fn get_column_batch_metadata(
        &self,
        collection: &str,
        index_name: &str,
    ) -> Result<Option<ColumnBatchMetadata>, CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        let Some(raw) = tx
            .get(&Self::column_batch_metadata_key(collection, index_name))
            .map_err(CassieError::from)?
        else {
            return Ok(None);
        };
        serde_json::from_slice(&raw)
            .map(Some)
            .map_err(|error| CassieError::Parse(format!("invalid column batch metadata: {error}")))
    }

    pub fn delete_column_batches(
        &self,
        collection: &str,
        index_name: &str,
    ) -> Result<(), CassieError> {
        let mut schema_tx = self.begin_schema_rw_tx()?;
        Self::delete_keys_with_prefix(
            &mut schema_tx,
            Self::column_batch_index_prefix(collection, index_name),
        )?;
        schema_tx
            .commit(WriteOptions::sync())
            .map_err(CassieError::from)?;

        let mut data_tx = self.begin_data_rw_tx()?;
        Self::delete_keys_with_prefix(
            &mut data_tx,
            Self::column_batch_index_prefix(collection, index_name),
        )?;
        data_tx
            .commit(WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub fn scan_column_batch_projected_rows(
        &self,
        collection: &str,
        batch_size: usize,
        fields: &[String],
        filter: Option<&RowFilter>,
        limit: Option<usize>,
    ) -> Result<Option<ColumnBatchScanOutcome>, CassieError> {
        let started = Instant::now();
        let Some(index) = self.covering_column_index(collection, fields)? else {
            return Ok(None);
        };
        let Some(metadata) = self.get_column_batch_metadata(collection, &index.name)? else {
            return Ok(None);
        };
        if metadata.fields.len() != index.normalized_fields().len()
            || metadata.segment_size != column_index_segment_size(&index)?
        {
            return Ok(None);
        }

        let wanted = fields
            .iter()
            .filter(|field| !field.eq_ignore_ascii_case("id") && !field.eq_ignore_ascii_case("_id"))
            .map(|field| field.to_ascii_lowercase())
            .collect::<BTreeSet<_>>();
        let available = metadata
            .fields
            .iter()
            .map(|field| field.to_ascii_lowercase())
            .collect::<BTreeSet<_>>();
        if !wanted.is_subset(&available) {
            return Ok(None);
        }

        let mut emitted = 0usize;
        let limit = limit.unwrap_or(usize::MAX);
        let batch_size = batch_size.max(1);
        let mut batches = Vec::new();
        let mut current = Vec::with_capacity(batch_size);
        let mut compressed_bytes = 0usize;
        let mut uncompressed_bytes = 0usize;
        let data_tx = self.begin_data_readonly_tx()?;
        for segment in &metadata.segments {
            let Some(raw) = data_tx
                .get(&Self::column_batch_segment_key(
                    collection,
                    &index.name,
                    segment.segment_id,
                ))
                .map_err(CassieError::from)?
            else {
                return Ok(None);
            };
            compressed_bytes = compressed_bytes.saturating_add(segment.codec.compressed_len);
            uncompressed_bytes = uncompressed_bytes.saturating_add(segment.codec.uncompressed_len);
            if segment
                .codec
                .checksum
                .as_ref()
                .is_some_and(|checksum| checksum != &checksum_hex(&raw))
            {
                return Ok(None);
            }
            let Ok(payload) = serde_json::from_slice::<ColumnBatchPayload>(&raw) else {
                return Ok(None);
            };
            if payload.encoding_version != COLUMN_BATCH_ENCODING_VERSION {
                return Ok(None);
            }
            if payload.codec_name != segment.codec.codec_name
                || payload.codec_version != segment.codec.codec_version
            {
                return Ok(None);
            }
            let Some(rows) = decode_column_batch_payload(&payload, segment.row_count)? else {
                return Ok(None);
            };
            for row in rows {
                if emitted >= limit {
                    break;
                }
                if !column_batch_row_matches(&row, filter) {
                    continue;
                }
                let payload = project_column_batch_row(&row, fields);
                current.push(DocumentRef {
                    id: row.row_id,
                    payload,
                });
                emitted += 1;
                if current.len() >= batch_size {
                    batches.push(current);
                    current = Vec::with_capacity(batch_size);
                }
            }
            if emitted >= limit {
                break;
            }
        }
        if !current.is_empty() {
            batches.push(current);
        }

        Ok(Some(ColumnBatchScanOutcome {
            batches,
            timings: MidgeScanTimings {
                scan: started.elapsed(),
                row_decode: Default::default(),
            },
            index_name: index.name,
            compressed_bytes,
            uncompressed_bytes,
        }))
    }

    pub(super) fn delete_keys_with_prefix(
        tx: &mut cntryl_midge::Transaction,
        prefix: Vec<u8>,
    ) -> Result<(), CassieError> {
        let mut scan = tx
            .scan(&Query::new().prefix(prefix.into()))
            .map_err(CassieError::from)?;
        let mut keys = Vec::new();
        while let Some((key, _)) = scan.next() {
            keys.push(key);
        }
        for key in keys {
            tx.delete(key).map_err(CassieError::from)?;
        }
        Ok(())
    }

    fn covering_column_index(
        &self,
        collection: &str,
        fields: &[String],
    ) -> Result<Option<IndexMeta>, CassieError> {
        let wanted = fields
            .iter()
            .filter(|field| !field.eq_ignore_ascii_case("id") && !field.eq_ignore_ascii_case("_id"))
            .map(|field| field.to_ascii_lowercase())
            .collect::<BTreeSet<_>>();
        if wanted.is_empty() {
            return Ok(None);
        }

        Ok(self
            .list_indexes()?
            .into_iter()
            .filter(|index| index.collection == collection && index.kind == IndexKind::Column)
            .find(|index| {
                let available = index
                    .normalized_fields()
                    .into_iter()
                    .map(|field| field.to_ascii_lowercase())
                    .collect::<BTreeSet<_>>();
                wanted.is_subset(&available)
            }))
    }
}

fn column_index_segment_size(index: &IndexMeta) -> Result<usize, CassieError> {
    let raw = index
        .options
        .get("segment_size")
        .map(String::as_str)
        .unwrap_or("1024")
        .trim();
    let parsed = raw
        .parse::<usize>()
        .map_err(|_| CassieError::Parse("invalid column index segment_size".to_string()))?;
    Ok(parsed.max(1))
}

fn encode_column_batch_payload(
    rows: Vec<ColumnBatchRow>,
    fields: &[String],
) -> Result<(ColumnBatchPayload, EncodedColumnBatch), CassieError> {
    let uncompressed = ColumnBatchPayload {
        encoding_version: COLUMN_BATCH_ENCODING_VERSION,
        codec_name: "uncompressed".to_string(),
        codec_version: COLUMN_BATCH_CODEC_VERSION,
        row_ids: Vec::new(),
        rows: rows.clone(),
        columns: Vec::new(),
    };
    let uncompressed_bytes =
        serde_json::to_vec(&uncompressed).map_err(|error| CassieError::Parse(error.to_string()))?;

    let rle = dictionary_rle_payload(&rows, fields);
    let rle_bytes =
        serde_json::to_vec(&rle).map_err(|error| CassieError::Parse(error.to_string()))?;

    let (payload, bytes) = if rle_bytes.len() < uncompressed_bytes.len() {
        (rle, rle_bytes)
    } else {
        (uncompressed, uncompressed_bytes)
    };
    let codec = EncodedColumnBatch {
        codec_name: payload.codec_name.clone(),
        codec_version: payload.codec_version,
        uncompressed_len: serde_json::to_vec(&rows)
            .map_err(|error| CassieError::Parse(error.to_string()))?
            .len(),
        compressed_len: bytes.len(),
        checksum: checksum_hex(&bytes),
    };
    Ok((payload, codec))
}

fn dictionary_rle_payload(rows: &[ColumnBatchRow], fields: &[String]) -> ColumnBatchPayload {
    let row_ids = rows
        .iter()
        .map(|row| row.row_id.clone())
        .collect::<Vec<_>>();
    let columns = fields
        .iter()
        .map(|field| ColumnBatchColumn {
            field: field.clone(),
            runs: value_runs(rows, field),
        })
        .collect();
    ColumnBatchPayload {
        encoding_version: COLUMN_BATCH_ENCODING_VERSION,
        codec_name: "dictionary_rle".to_string(),
        codec_version: COLUMN_BATCH_CODEC_VERSION,
        row_ids,
        rows: Vec::new(),
        columns,
    }
}

fn value_runs(rows: &[ColumnBatchRow], field: &str) -> Vec<ColumnBatchValueRun> {
    let mut runs: Vec<ColumnBatchValueRun> = Vec::new();
    for row in rows {
        let value = row
            .values
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(field))
            .map(|(_, value)| value.clone())
            .unwrap_or(serde_json::Value::Null);
        if let Some(last) = runs.last_mut() {
            if last.value == value {
                last.len += 1;
                continue;
            }
        }
        runs.push(ColumnBatchValueRun { value, len: 1 });
    }
    runs
}

fn decode_column_batch_payload(
    payload: &ColumnBatchPayload,
    row_count: usize,
) -> Result<Option<Vec<ColumnBatchRow>>, CassieError> {
    match (payload.codec_name.as_str(), payload.codec_version) {
        ("uncompressed", COLUMN_BATCH_CODEC_VERSION) => Ok(Some(payload.rows.clone())),
        ("dictionary_rle", COLUMN_BATCH_CODEC_VERSION) => {
            decode_dictionary_rle_payload(payload, row_count)
        }
        _ => Ok(None),
    }
}

fn decode_dictionary_rle_payload(
    payload: &ColumnBatchPayload,
    row_count: usize,
) -> Result<Option<Vec<ColumnBatchRow>>, CassieError> {
    if payload.row_ids.len() != row_count {
        return Ok(None);
    }
    let mut rows = payload
        .row_ids
        .iter()
        .map(|row_id| ColumnBatchRow {
            row_id: row_id.clone(),
            values: BTreeMap::new(),
        })
        .collect::<Vec<_>>();

    for column in &payload.columns {
        let mut offset = 0usize;
        for run in &column.runs {
            if run.len == 0 || offset.saturating_add(run.len) > row_count {
                return Ok(None);
            }
            for row in rows.iter_mut().skip(offset).take(run.len) {
                row.values.insert(column.field.clone(), run.value.clone());
            }
            offset += run.len;
        }
        if offset != row_count {
            return Ok(None);
        }
    }

    Ok(Some(rows))
}

fn checksum_hex(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn column_values(
    payload: &serde_json::Value,
    fields: &[String],
) -> BTreeMap<String, serde_json::Value> {
    let object = payload.as_object();
    fields
        .iter()
        .map(|field| {
            let value = object
                .and_then(|object| {
                    object.get(field).or_else(|| {
                        object
                            .iter()
                            .find(|(name, _)| name.eq_ignore_ascii_case(field))
                            .map(|(_, value)| value)
                    })
                })
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            (field.clone(), value)
        })
        .collect()
}

fn column_batch_row_matches(row: &ColumnBatchRow, filter: Option<&RowFilter>) -> bool {
    let Some(filter) = filter else {
        return true;
    };
    row.values
        .iter()
        .find(|(field, _)| field.eq_ignore_ascii_case(&filter.field))
        .is_some_and(|(_, value)| value == &filter.value)
}

fn project_column_batch_row(row: &ColumnBatchRow, fields: &[String]) -> serde_json::Value {
    let mut object = serde_json::Map::new();
    for field in fields {
        if field.eq_ignore_ascii_case("id") || field.eq_ignore_ascii_case("_id") {
            continue;
        }
        let value = row
            .values
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(field))
            .map(|(_, value)| value.clone())
            .unwrap_or(serde_json::Value::Null);
        object.insert(field.clone(), value);
    }
    serde_json::Value::Object(object)
}
