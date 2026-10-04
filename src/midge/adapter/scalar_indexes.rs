use super::{
    check_document_write_failure_point, key_encoding, CassieError, DataType,
    DocumentWriteFailurePoint, Midge, RowDecode, RowSchema,
};
use crate::catalog::{IndexKind, IndexMeta};
use crate::executor::filter;
use crate::sql::ast::Expr;
use crate::types::Value;
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};

mod codec;
mod reservation_owner;
mod scan;

use self::codec::encode_covering_fields;

#[derive(Debug, Clone)]
pub(crate) struct ScalarIndexBound {
    pub value: serde_json::Value,
    pub inclusive: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ScalarIndexScanRequest {
    pub equality_prefix: Vec<serde_json::Value>,
    pub lower_bound: Option<ScalarIndexBound>,
    pub upper_bound: Option<ScalarIndexBound>,
    pub reverse: bool,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone)]
pub(crate) struct ScalarIndexScanHit {
    pub id: String,
    pub fields: serde_json::Map<String, serde_json::Value>,
}

type ScalarIndexEntry = (Vec<u8>, Vec<u8>);
const SCALAR_INDEX_BUILD_WRITE_BATCH_SIZE: usize = 5_000;
const SCALAR_INDEX_FLUSH_INTERVAL_BATCHES: usize = 4;

fn scalar_index_build_ranges(item_count: usize) -> impl Iterator<Item = std::ops::Range<usize>> {
    (0..item_count)
        .step_by(SCALAR_INDEX_BUILD_WRITE_BATCH_SIZE)
        .map(move |start| {
            start
                ..start
                    .saturating_add(SCALAR_INDEX_BUILD_WRITE_BATCH_SIZE)
                    .min(item_count)
        })
}

fn should_flush_scalar_index_batch(batch_number: usize, batch_count: usize) -> bool {
    batch_number.is_multiple_of(SCALAR_INDEX_FLUSH_INTERVAL_BATCHES) || batch_number == batch_count
}

impl Midge {
    pub(crate) fn sync_scalar_indexes_for_document(
        tx: &mut cntryl_midge::Transaction,
        id: &str,
        old_payload: Option<&serde_json::Value>,
        new_payload: Option<&serde_json::Value>,
        indexes: &[IndexMeta],
        row_schema: &RowSchema,
    ) -> Result<(usize, usize), CassieError> {
        let mut deletes = 0usize;
        let mut puts = 0usize;

        for index in indexes {
            let old_entry = match old_payload {
                Some(payload) => Self::scalar_index_entry(index, id, payload, row_schema)?,
                None => None,
            };
            let new_entry = match new_payload {
                Some(payload) => Self::scalar_index_entry(index, id, payload, row_schema)?,
                None => None,
            };

            match (old_entry.as_ref(), new_entry.as_ref()) {
                (Some((old_key, old_value)), Some((new_key, new_value))) if old_key == new_key => {
                    if old_value != new_value {
                        tx.put(new_key.clone(), new_value.clone(), None)
                            .map_err(CassieError::from)?;
                        puts += 1;
                    }
                }
                _ => {
                    if let Some((old_key, _)) = old_entry {
                        tx.delete(old_key).map_err(CassieError::from)?;
                        deletes += 1;
                    }
                    if let Some((new_key, new_value)) = new_entry {
                        tx.put(new_key, new_value, None)
                            .map_err(CassieError::from)?;
                        puts += 1;
                    }
                }
            }
        }

        check_document_write_failure_point(DocumentWriteFailurePoint::ScalarIndex)?;

        Ok((deletes, puts))
    }

    pub(crate) fn rebuild_scalar_indexes_for_collection(
        &self,
        collection: &str,
    ) -> Result<(), CassieError> {
        for index in self.list_indexes()?.into_iter().filter(|index| {
            index.collection == collection && Self::scalar_index_supports_storage(index)
        }) {
            self.rebuild_scalar_index_for_index(&index)?;
        }
        Ok(())
    }

    pub(crate) fn rebuild_scalar_index_for_index(
        &self,
        index: &IndexMeta,
    ) -> Result<(), CassieError> {
        if !Self::scalar_index_supports_storage(index) {
            self.delete_scalar_index_data(&index.collection, &index.name)?;
            return Ok(());
        }

        let rows = self.scan_rows_for_rebuild(&index.collection, RowDecode::Full)?;
        let row_schema = self.row_schema(&index.collection)?;
        let (relation_id, index_id) = Self::scalar_index_storage_ids(index)?;
        let mut tx = self.begin_data_rw_tx_for(&index.collection)?;
        Self::delete_keys_with_prefix(
            &mut tx,
            Self::scalar_index_data_prefix(relation_id, index_id),
        )?;

        for row in rows {
            if let Some((key, value)) =
                Self::scalar_index_entry(index, &row.id, &row.payload, &row_schema)?
            {
                tx.put(key, value, None).map_err(CassieError::from)?;
            }
        }

        tx.commit(self.write_options_sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub(super) fn rebuild_prepared_scalar_index_for_index(
        &self,
        index: &IndexMeta,
    ) -> Result<(), CassieError> {
        if !Self::scalar_index_supports_storage(index) {
            return Ok(());
        }
        if self.get_index(&index.collection, &index.name)?.is_some() {
            return self.rebuild_scalar_index_for_index(index);
        }

        let rows = self.scan_rows_for_rebuild(&index.collection, RowDecode::Full)?;
        let row_schema = self.row_schema(&index.collection)?;
        if index.unique {
            self.validate_unique_index_rows(index, &rows)?;
            self.delete_data_keys_with_prefix_in_batches(
                &index.collection,
                &key_encoding::unique_scalar_index_reservation_prefix(
                    &index.collection,
                    &index.name,
                ),
            )?;
        }
        let (relation_id, index_id) = Self::scalar_index_storage_ids(index)?;
        let prefix = Self::scalar_index_data_prefix(relation_id, index_id);
        self.delete_prepared_scalar_index_data_in_batches(&index.collection, &prefix)?;

        let batch_count = rows.len().div_ceil(SCALAR_INDEX_BUILD_WRITE_BATCH_SIZE);
        for (batch_index, range) in scalar_index_build_ranges(rows.len()).enumerate() {
            let mut tx = self.begin_data_rw_tx_for(&index.collection)?;
            for row in &rows[range] {
                let canonical = scalar_index_canonical_payload(&row_schema, &row.payload);
                let reservation_values = if index.unique
                    && Self::payload_matches_scalar_index_predicate(index, &canonical, &row_schema)?
                {
                    Self::scalar_index_key_values(index, &canonical, &row_schema)?
                } else {
                    None
                };
                if let Some(values) = reservation_values {
                    let reservation = key_encoding::unique_scalar_index_reservation_key(
                        &index.collection,
                        &index.name,
                        &values,
                    )?;
                    tx.put(reservation, row.id.as_bytes().to_vec(), None)
                        .map_err(CassieError::from)?;
                }
                if let Some((key, value)) =
                    Self::scalar_index_entry(index, &row.id, &canonical, &row_schema)?
                {
                    tx.put(key, value, None).map_err(CassieError::from)?;
                }
            }
            tx.commit(self.write_options_sync())
                .map_err(CassieError::from)?;
            if should_flush_scalar_index_batch(batch_index + 1, batch_count) {
                self.flush_data_family_for_collection(&index.collection)?;
            }
        }
        Ok(())
    }

    fn validate_unique_index_rows(
        &self,
        index: &IndexMeta,
        rows: &[crate::midge::adapter::DocumentRef],
    ) -> Result<(), CassieError> {
        let mut owners = HashMap::<Vec<u8>, String>::new();
        let row_schema = self.row_schema(&index.collection)?;
        for row in rows {
            let canonical = scalar_index_canonical_payload(&row_schema, &row.payload);
            if !Self::payload_matches_scalar_index_predicate(index, &canonical, &row_schema)? {
                continue;
            }
            let Some(values) = Self::scalar_index_key_values(index, &canonical, &row_schema)?
            else {
                continue;
            };
            let key = key_encoding::unique_scalar_index_reservation_key(
                &index.collection,
                &index.name,
                &values,
            )?;
            if owners.insert(key, row.id.clone()).is_some() {
                return Err(index.unique_violation(&index.collection));
            }
        }
        Ok(())
    }

    pub(super) fn delete_data_keys_with_prefix_in_batches(
        &self,
        collection: &str,
        prefix: &[u8],
    ) -> Result<(), CassieError> {
        let mut batches_since_flush = 0;
        loop {
            let keys = self
                .raw_scan_prefix_page_for_collection(
                    collection,
                    prefix,
                    SCALAR_INDEX_BUILD_WRITE_BATCH_SIZE,
                )?
                .into_iter()
                .map(|(key, _)| key)
                .collect::<Vec<_>>();
            if keys.is_empty() {
                if batches_since_flush > 0 {
                    self.flush_data_family_for_collection(collection)?;
                }
                return Ok(());
            }
            let mut tx = self.begin_data_rw_tx_for(collection)?;
            for key in keys {
                tx.delete(key).map_err(CassieError::from)?;
            }
            tx.commit(self.write_options_sync())
                .map_err(CassieError::from)?;
            batches_since_flush += 1;
            if batches_since_flush == SCALAR_INDEX_FLUSH_INTERVAL_BATCHES {
                self.flush_data_family_for_collection(collection)?;
                batches_since_flush = 0;
            }
        }
    }

    fn delete_prepared_scalar_index_data_in_batches(
        &self,
        collection: &str,
        prefix: &[u8],
    ) -> Result<(), CassieError> {
        self.delete_data_keys_with_prefix_in_batches(collection, prefix)
    }

    pub(crate) fn delete_scalar_index_data(
        &self,
        collection: &str,
        index_name: &str,
    ) -> Result<(), CassieError> {
        let Some(index) = self.get_index(collection, index_name)? else {
            return Ok(());
        };
        let (relation_id, index_id) = Self::scalar_index_storage_ids(&index)?;
        let prefix = Self::scalar_index_data_prefix(relation_id, index_id);
        self.delete_prepared_scalar_index_data_in_batches(&index.collection, &prefix)?;
        Ok(())
    }

    fn scalar_index_supports_storage(index: &IndexMeta) -> bool {
        index.kind == IndexKind::Scalar
            && (!index.normalized_fields().is_empty() || !index.normalized_expressions().is_empty())
    }

    fn scalar_index_storage_ids(index: &IndexMeta) -> Result<(u64, u64), CassieError> {
        let relation_id = index.relation_id().ok_or_else(|| {
            CassieError::Parse(format!("index '{}' is missing its relation id", index.name))
        })?;
        let index_id = index.storage_id().ok_or_else(|| {
            CassieError::Parse(format!("index '{}' is missing its storage id", index.name))
        })?;
        Ok((relation_id, index_id))
    }

    fn scalar_index_entry(
        index: &IndexMeta,
        id: &str,
        payload: &serde_json::Value,
        row_schema: &RowSchema,
    ) -> Result<Option<ScalarIndexEntry>, CassieError> {
        if !Self::scalar_index_supports_storage(index)
            || !Self::payload_matches_scalar_index_predicate(index, payload, row_schema)?
        {
            return Ok(None);
        }

        let Some(key_values) = Self::scalar_index_key_values(index, payload, row_schema)? else {
            return Ok(None);
        };
        let (relation_id, index_id) = Self::scalar_index_storage_ids(index)?;
        let key = key_encoding::scalar_index_entry_key(relation_id, index_id, &key_values, id)?;
        let stored_fields = Self::scalar_index_stored_fields(index, payload);
        let value = if stored_fields.is_empty() {
            Vec::new()
        } else {
            encode_covering_fields(&stored_fields)
        };
        Ok(Some((key, value)))
    }

    pub(crate) fn scalar_index_key_values(
        index: &IndexMeta,
        payload: &serde_json::Value,
        row_schema: &RowSchema,
    ) -> Result<Option<Vec<serde_json::Value>>, CassieError> {
        let mut values = Vec::new();
        for field in index.normalized_fields() {
            let Some(value) = payload.get(&field) else {
                return Ok(None);
            };
            if value.is_null()
                && !row_schema.fields.iter().any(|field_meta| {
                    field_meta.name == field && matches!(field_meta.data_type, DataType::Json)
                })
            {
                return Ok(None);
            }
            values.push(value.clone());
        }

        let expressions = index.normalized_expressions();
        if expressions.is_empty() {
            return Ok(Some(values));
        }

        let row = payload_to_row(payload, row_schema);
        let user_functions = HashMap::new();
        for raw_expression in expressions {
            let expression = Self::scalar_index_expression(&index.name, &raw_expression)?;
            let value = filter::evaluate_expr_value(
                &row,
                &expression,
                &[],
                None,
                &user_functions,
                None,
                None,
            )
            .map_err(|error| {
                CassieError::Parse(format!(
                    "invalid scalar index expression evaluation for '{}': {error}",
                    index.name
                ))
            })?;
            if matches!(value, Value::Null) {
                return Ok(None);
            }
            values.push(query_value_to_json(value)?);
        }

        Ok(Some(values))
    }

    fn scalar_index_expression(index_name: &str, raw: &str) -> Result<Expr, CassieError> {
        serde_json::from_str(raw).map_err(|error| {
            CassieError::Parse(format!(
                "invalid scalar index expression for '{index_name}': {error}"
            ))
        })
    }

    fn scalar_index_stored_fields(
        index: &IndexMeta,
        payload: &serde_json::Value,
    ) -> BTreeMap<String, serde_json::Value> {
        let mut fields = BTreeMap::new();
        for field in index
            .normalized_fields()
            .into_iter()
            .chain(index.normalized_include_fields())
        {
            if let Some(value) = payload.get(&field) {
                fields.entry(field).or_insert_with(|| value.clone());
            }
        }
        fields
    }

    pub(crate) fn payload_matches_scalar_index_predicate(
        index: &IndexMeta,
        payload: &serde_json::Value,
        row_schema: &RowSchema,
    ) -> Result<bool, CassieError> {
        let Some(raw_predicate) = index.predicate.as_ref() else {
            return Ok(true);
        };
        let predicate: Expr = serde_json::from_str(raw_predicate).map_err(|error| {
            CassieError::Parse(format!(
                "invalid scalar index predicate for '{}': {error}",
                index.name
            ))
        })?;
        let row = payload_to_row(payload, row_schema);
        let matched = !filter::filter_rows(vec![row], &predicate, &[], None, &HashMap::new(), None)
            .map_err(|error| {
                CassieError::Parse(format!(
                    "invalid scalar index predicate evaluation: {error}"
                ))
            })?
            .is_empty();
        Ok(matched)
    }
}

/// Returns the payload with each field in the form its row blob decodes to.
///
/// Row blobs decode `FLOAT` columns as floats and `DATE`/`TIME`/`TIMESTAMP`
/// columns in their canonical text form, so index backfill, delete-side
/// maintenance, and index probes all encode those keys canonically. Incoming
/// write payloads can still carry integer-shaped numbers for `FLOAT` columns
/// or non-canonical temporal text (offsets, short fractions, a space
/// separator); canonicalizing them keeps incrementally maintained keys
/// identical to the stored values.
pub(crate) fn scalar_index_canonical_payload<'a>(
    row_schema: &RowSchema,
    payload: &'a serde_json::Value,
) -> Cow<'a, serde_json::Value> {
    let Some(object) = payload.as_object() else {
        return Cow::Borrowed(payload);
    };
    let canonical_fields = row_schema
        .active_fields_by_id()
        .into_iter()
        .filter_map(|field| {
            let value = object.get(&field.name)?;
            let canonical = canonical_field_value(&field.data_type, value)?;
            (&canonical != value).then(|| (field.name.clone(), canonical))
        })
        .collect::<Vec<_>>();
    if canonical_fields.is_empty() {
        return Cow::Borrowed(payload);
    }

    let mut canonical = object.clone();
    for (field, value) in canonical_fields {
        canonical.insert(field, value);
    }
    Cow::Owned(serde_json::Value::Object(canonical))
}

pub(crate) fn canonical_field_value(
    data_type: &DataType,
    value: &serde_json::Value,
) -> Option<serde_json::Value> {
    let canonical_text = match data_type {
        DataType::Uuid => {
            return value
                .as_str()
                .and_then(|value| uuid::Uuid::parse_str(value).ok())
                .map(|value| serde_json::Value::String(value.to_string()));
        }
        DataType::Bytea => {
            return value
                .as_str()
                .and_then(|value| crate::midge::row_blob::canonical_bytea_text(value).ok())
                .map(serde_json::Value::String);
        }
        DataType::Float => {
            return value
                .as_number()
                .filter(|number| !number.is_f64())
                .and_then(serde_json::Number::as_f64)
                .and_then(serde_json::Number::from_f64)
                .map(serde_json::Value::Number);
        }
        DataType::Char { .. } => {
            return value
                .as_str()
                .map(crate::types::char_text::canonical_char_text)
                .map(|text| serde_json::Value::String(text.to_string()));
        }
        DataType::Date => crate::types::temporal::canonical_date,
        DataType::Time => crate::types::temporal::canonical_time,
        DataType::Timestamp => crate::types::temporal::canonical_timestamp,
        _ => return None,
    };
    canonical_text(value.as_str()?)
        .ok()
        .map(serde_json::Value::String)
}

fn payload_to_row(payload: &serde_json::Value, row_schema: &RowSchema) -> Vec<(String, Value)> {
    let Some(object) = payload.as_object() else {
        return Vec::new();
    };
    object
        .iter()
        .map(|(field, value)| {
            let data_type = row_schema
                .fields
                .iter()
                .find(|field_meta| field_meta.name == *field)
                .map(|field_meta| &field_meta.data_type);
            (field.clone(), json_to_query_value(value, data_type))
        })
        .collect()
}

fn json_to_query_value(value: &serde_json::Value, data_type: Option<&DataType>) -> Value {
    if matches!(data_type, Some(DataType::Json)) && (value.is_null() || value.is_string()) {
        return Value::Json(value.clone());
    }
    if value.is_null() {
        return Value::Null;
    }
    if let Some(value) = value.as_str() {
        return Value::String(value.to_string());
    }
    if let Some(value) = value.as_bool() {
        return Value::Bool(value);
    }
    if let Some(value) = value.as_i64() {
        return Value::Int64(value);
    }
    if let Some(value) = value.as_u64().and_then(|value| i64::try_from(value).ok()) {
        return Value::Int64(value);
    }
    if let Some(value) = value.as_f64() {
        return Value::Float64(value);
    }
    Value::Json(value.clone())
}

fn query_value_to_json(value: Value) -> Result<serde_json::Value, CassieError> {
    match value {
        Value::Null => Ok(serde_json::Value::Null),
        Value::Bool(value) => Ok(serde_json::Value::Bool(value)),
        Value::Int64(value) => Ok(serde_json::Value::Number(value.into())),
        Value::Float64(value) => serde_json::Number::from_f64(value)
            .map(serde_json::Value::Number)
            .ok_or_else(|| {
                CassieError::Unsupported(
                    "non-finite scalar index expression values are not supported".to_string(),
                )
            }),
        Value::String(value) => Ok(serde_json::Value::String(value)),
        Value::Vector(value) => value
            .values
            .into_iter()
            .map(|value| {
                serde_json::Number::from_f64(f64::from(value))
                    .map(serde_json::Value::Number)
                    .ok_or_else(|| {
                        CassieError::Unsupported(
                            "non-finite scalar index expression values are not supported"
                                .to_string(),
                        )
                    })
            })
            .collect::<Result<_, _>>()
            .map(serde_json::Value::Array),
        Value::Json(value) => Ok(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_bound_prepared_scalar_index_publication_batches() {
        // Arrange
        let entry_count = 10_001;

        // Act
        let batches = scalar_index_build_ranges(entry_count)
            .map(|range| range.len())
            .collect::<Vec<_>>();

        // Assert
        assert_eq!(batches, vec![5_000, 5_000, 1]);
    }

    #[test]
    fn should_flush_scalar_index_batches_at_bounded_intervals() {
        // Arrange
        let exact_interval_batch_count = 20;
        let partial_interval_batch_count = 10;

        // Act
        let exact_interval_flushes = (1..=exact_interval_batch_count)
            .filter(|batch_number| {
                should_flush_scalar_index_batch(*batch_number, exact_interval_batch_count)
            })
            .collect::<Vec<_>>();
        let partial_interval_flushes = (1..=partial_interval_batch_count)
            .filter(|batch_number| {
                should_flush_scalar_index_batch(*batch_number, partial_interval_batch_count)
            })
            .collect::<Vec<_>>();

        // Assert
        assert_eq!(exact_interval_flushes, vec![4, 8, 12, 16, 20]);
        assert_eq!(partial_interval_flushes, vec![4, 8, 10]);
    }
}
