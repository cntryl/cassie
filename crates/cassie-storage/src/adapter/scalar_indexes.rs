use super::StorageValueHooks;
use super::{
    check_document_write_failure_point, key_encoding, CassieError, DataType,
    DocumentWriteFailurePoint, Midge, RowDecode, RowSchema,
};
use crate::{ScalarExpressionEvaluationError, ScalarExpressionEvaluator};
use cassie_core::catalog::declarations::IndexKind;
use cassie_core::types::{self, Value};
use cassie_sql::catalog_declarations::IndexMeta;
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};

mod codec;
mod reservation_owner;
mod scan;

use self::codec::encode_covering_fields;

#[derive(Debug, Clone)]
pub struct ScalarIndexBound {
    pub value: serde_json::Value,
    pub inclusive: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ScalarIndexScanRequest {
    pub equality_prefix: Vec<serde_json::Value>,
    pub lower_bound: Option<ScalarIndexBound>,
    pub upper_bound: Option<ScalarIndexBound>,
    pub reverse: bool,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct ScalarIndexScanHit {
    pub id: String,
    pub fields: serde_json::Map<String, serde_json::Value>,
}

type EvaluateScalarExpression = fn(&str, &str, &[(String, Value)]) -> Result<Value, CassieError>;

struct ScalarExpressionHook<'a> {
    index_name: &'a str,
    evaluate: EvaluateScalarExpression,
}

impl ScalarExpressionEvaluator for ScalarExpressionHook<'_> {
    type Error = CassieError;

    fn evaluate(
        &self,
        stored_expression: &str,
        row: &[(String, Value)],
    ) -> Result<Value, CassieError> {
        (self.evaluate)(self.index_name, stored_expression, row)
    }
}

pub(super) fn scalar_expression_evaluator<'a>(
    index_name: &'a str,
    hooks: &StorageValueHooks,
) -> impl ScalarExpressionEvaluator<Error = CassieError> + 'a {
    ScalarExpressionHook {
        index_name,
        evaluate: hooks.evaluate_scalar_expression,
    }
}

impl From<ScalarExpressionEvaluationError<CassieError>> for CassieError {
    fn from(error: ScalarExpressionEvaluationError<CassieError>) -> Self {
        match error {
            ScalarExpressionEvaluationError::Evaluation(error) => error,
            ScalarExpressionEvaluationError::UnsupportedValue(message) => {
                Self::Unsupported(message)
            }
        }
    }
}

pub(super) fn unique_index_violation(index: &IndexMeta, collection: &str) -> CassieError {
    let details = index.unique_violation_details(collection);
    CassieError::UniqueViolation {
        table: details.table,
        column: details.column,
        constraint: details.constraint,
    }
}

type ScalarIndexEntry = (Vec<u8>, Vec<u8>);
const SCALAR_INDEX_BUILD_WRITE_BATCH_SIZE: usize = 5_000;
const SCALAR_INDEX_FLUSH_INTERVAL_BATCHES: usize = 8;

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
        tx: &mut crate::adapter::AdapterTransaction,
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
                Some(payload) => {
                    Self::scalar_index_entry(index, id, payload, row_schema, tx.value_hooks())?
                }
                None => None,
            };
            let new_entry = match new_payload {
                Some(payload) => {
                    Self::scalar_index_entry(index, id, payload, row_schema, tx.value_hooks())?
                }
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

    /// # Errors
    ///
    /// Returns an error when the collection's scalar indexes cannot be read or rebuilt.
    pub fn rebuild_scalar_indexes_for_collection(
        &self,
        collection: &str,
    ) -> Result<(), CassieError> {
        let collection = self.canonical_collection_name(collection);
        for index in self.list_indexes()?.into_iter().filter(|index| {
            index.collection == collection && Self::scalar_index_supports_storage(index)
        }) {
            self.rebuild_scalar_index_for_index(&index)?;
        }
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when the current scalar index cannot be read or rebuilt.
    pub fn rebuild_scalar_index_for_index(&self, index: &IndexMeta) -> Result<(), CassieError> {
        let write_gate = self.collection_write_gate(&index.collection);
        let _write_guard = write_gate.lock();
        let Some(current_index) = self.get_index(&index.collection, &index.name)? else {
            return Ok(());
        };
        self.rebuild_scalar_index_for_index_locked(&current_index)
    }

    fn rebuild_scalar_index_for_index_locked(&self, index: &IndexMeta) -> Result<(), CassieError> {
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
            if let Some((key, value)) = Self::scalar_index_entry(
                index,
                &row.id,
                &row.payload,
                &row_schema,
                &self.value_hooks(),
            )? {
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
                &key_encoding::unique_index_reservation_prefix_for(
                    &index.collection,
                    index
                        .reservation_identity_details()
                        .map_err(CassieError::Parse)?,
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
                    && Self::payload_matches_scalar_index_predicate(
                        index,
                        &canonical,
                        &row_schema,
                        self.value_hooks().evaluate_scalar_predicate,
                    )? {
                    let evaluator = scalar_expression_evaluator(&index.name, &self.value_hooks());
                    Self::scalar_index_key_values(index, &canonical, &row_schema, &evaluator)?
                } else {
                    None
                };
                if let Some(values) = reservation_values {
                    let reservation = key_encoding::unique_index_reservation_key_for(
                        &index.collection,
                        index
                            .reservation_identity_details()
                            .map_err(CassieError::Parse)?,
                        &values,
                    )?;
                    tx.put(reservation, row.id.as_bytes().to_vec(), None)
                        .map_err(CassieError::from)?;
                }
                if let Some((key, value)) = Self::scalar_index_entry(
                    index,
                    &row.id,
                    &canonical,
                    &row_schema,
                    &self.value_hooks(),
                )? {
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
        rows: &[crate::adapter::DocumentRef],
    ) -> Result<(), CassieError> {
        let mut owners = HashMap::<Vec<u8>, String>::new();
        let row_schema = self.row_schema(&index.collection)?;
        for row in rows {
            let canonical = scalar_index_canonical_payload(&row_schema, &row.payload);
            if !Self::payload_matches_scalar_index_predicate(
                index,
                &canonical,
                &row_schema,
                self.value_hooks().evaluate_scalar_predicate,
            )? {
                continue;
            }
            let evaluator = scalar_expression_evaluator(&index.name, &self.value_hooks());
            let Some(values) =
                Self::scalar_index_key_values(index, &canonical, &row_schema, &evaluator)?
            else {
                continue;
            };
            let key = key_encoding::unique_index_reservation_key_for(
                &index.collection,
                index
                    .reservation_identity_details()
                    .map_err(CassieError::Parse)?,
                &values,
            )?;
            if owners.insert(key, row.id.clone()).is_some() {
                return Err(unique_index_violation(index, &index.collection));
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

    /// # Errors
    ///
    /// Returns an error if the index cannot be inspected or its data removed.
    pub fn delete_scalar_index_data(
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
        hooks: &StorageValueHooks,
    ) -> Result<Option<ScalarIndexEntry>, CassieError> {
        if !Self::scalar_index_supports_storage(index)
            || !Self::payload_matches_scalar_index_predicate(
                index,
                payload,
                row_schema,
                hooks.evaluate_scalar_predicate,
            )?
        {
            return Ok(None);
        }

        let evaluator = scalar_expression_evaluator(&index.name, hooks);
        let Some(key_values) =
            Self::scalar_index_key_values(index, payload, row_schema, &evaluator)?
        else {
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

    /// # Errors
    ///
    /// Returns a typed expression-evaluation or persisted key-conversion error.
    pub fn scalar_index_key_values<E>(
        index: &IndexMeta,
        payload: &serde_json::Value,
        row_schema: &RowSchema,
        evaluator: &impl ScalarExpressionEvaluator<Error = E>,
    ) -> Result<Option<Vec<serde_json::Value>>, ScalarExpressionEvaluationError<E>> {
        let mut values = Vec::new();
        for field in index.normalized_fields() {
            let Some(value) = payload.get(&field) else {
                return Ok(None);
            };
            if value.is_null()
                && !index.nulls_not_distinct()
                && !row_schema.fields.iter().any(|field_meta| {
                    field_meta.name == field
                        && matches!(field_meta.data_type, DataType::Json | DataType::Jsonb)
                })
            {
                return Ok(None);
            }
            values.push(index_key_field_value(row_schema, &field, value));
        }

        let expressions = index.normalized_expressions();
        if expressions.is_empty() {
            return Ok(Some(values));
        }

        let row = payload_to_row(payload, row_schema);
        let expression_values = crate::evaluate_expression_index_values(
            &expressions,
            &row,
            index.nulls_not_distinct(),
            evaluator,
        )?;
        let Some(expression_values) = expression_values else {
            return Ok(None);
        };
        values.extend(expression_values);

        Ok(Some(values))
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

    /// # Errors
    ///
    /// Returns a definition error or the original error from the predicate evaluator.
    pub fn payload_matches_scalar_index_predicate<E>(
        index: &IndexMeta,
        payload: &serde_json::Value,
        row_schema: &RowSchema,
        evaluate: impl FnOnce(&str, &str, Vec<(String, Value)>) -> Result<bool, E>,
    ) -> Result<bool, E>
    where
        E: From<cassie_sql::definition_guard::DefinitionError>,
    {
        cassie_sql::definition_guard::index(
            index.definition_semantics_version,
            &index.expressions,
            index.predicate.as_deref(),
        )?;
        let Some(raw_predicate) = index.predicate.as_ref() else {
            return Ok(true);
        };
        evaluate(
            &index.name,
            raw_predicate,
            payload_to_row(payload, row_schema),
        )
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

/// The key form of a stored field value. `numeric` and `real` carriers become
/// order-preserving, value-canonical key text (equal values key equally and the
/// byte order is the numeric order); every other value is used as stored.
#[doc(hidden)]
#[must_use]
pub fn index_key_field_value(
    row_schema: &RowSchema,
    field: &str,
    value: &serde_json::Value,
) -> serde_json::Value {
    use types::extended::{index_key, ExtendedType};
    let Some(field_meta) = row_schema
        .fields
        .iter()
        .find(|field_meta| field_meta.name == field)
    else {
        return value.clone();
    };
    match &field_meta.data_type {
        DataType::Extended(ExtendedType::Numeric(_)) => index_key::numeric_index_carrier(value),
        DataType::Extended(ExtendedType::Real) => index_key::real_stored_index_carrier(value),
        _ => None,
    }
    .unwrap_or_else(|| value.clone())
}

#[doc(hidden)]
#[must_use]
pub fn canonical_field_value(
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
                .and_then(|value| crate::row_blob::canonical_bytea_text(value).ok())
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
                .map(types::char_text::canonical_char_text)
                .map(|text| serde_json::Value::String(text.to_string()));
        }
        DataType::Date => types::temporal::canonical_date,
        DataType::Time => types::temporal::canonical_time,
        DataType::Timestamp => types::temporal::canonical_timestamp,
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
    if data_type == Some(&DataType::Jsonb)
        || (matches!(data_type, Some(DataType::Json)) && (value.is_null() || value.is_string()))
    {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{mpsc, Arc};
    use std::thread;
    use std::time::Duration;

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
        assert_eq!(exact_interval_flushes, vec![8, 16, 20]);
        assert_eq!(partial_interval_flushes, vec![8, 10]);
    }

    #[test]
    fn should_serialize_scalar_index_rebuild_with_collection_writes() {
        // Arrange
        let midge = Arc::new(Midge::new_in_memory("postgres").expect("create Midge"));
        midge
            .create_collection(
                "scalar_reindex_gate",
                super::super::Schema {
                    fields: vec![super::super::FieldSchema {
                        name: "value".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    }],
                },
            )
            .expect("create collection");
        midge
            .put_document(
                "scalar_reindex_gate",
                Some("row-1".to_string()),
                serde_json::json!({"value": "alpha"}),
            )
            .expect("seed collection row");
        let index = IndexMeta {
            definition_semantics_version: 0,
            collection: "scalar_reindex_gate".to_string(),
            name: "scalar_reindex_gate_value".to_string(),
            field: "value".to_string(),
            fields: vec!["value".to_string()],
            expressions: Vec::new(),
            include_fields: Vec::new(),
            predicate: None,
            kind: IndexKind::Scalar,
            unique: false,
            options: std::collections::BTreeMap::new(),
        };
        midge.put_index(&index).expect("publish scalar index");
        let index = midge
            .get_index("scalar_reindex_gate", "scalar_reindex_gate_value")
            .expect("read scalar index")
            .expect("published scalar index");
        let collection = midge.canonical_collection_name(&index.collection);
        let gate = midge.collection_write_gate(&collection);
        let write_guard = gate.lock();
        let (started_tx, started_rx) = mpsc::channel();
        let (completed_tx, completed_rx) = mpsc::channel();
        let worker_midge = Arc::clone(&midge);
        let worker_gate = Arc::clone(&gate);

        let worker = thread::spawn(move || {
            started_tx
                .send(worker_gate.try_lock().is_none())
                .expect("signal that the collection gate is held");
            let result = worker_midge.rebuild_scalar_index_for_index(&index);
            completed_tx
                .send(result)
                .expect("signal scalar index rebuild finish");
        });
        let gate_was_held = started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("rebuild worker should reach the gate check");

        // Act
        let early_result = completed_rx.recv_timeout(Duration::from_millis(100)).ok();
        let blocked_while_write_guard_held = early_result.is_none();
        drop(write_guard);
        let result = early_result.unwrap_or_else(|| {
            completed_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("rebuild should finish after the collection gate is released")
        });
        worker.join().expect("join scalar index rebuild worker");

        // Assert
        assert!(gate_was_held);
        assert!(blocked_while_write_guard_held);
        assert!(result.is_ok());
    }

    #[test]
    fn should_not_rebuild_scalar_index_from_metadata_removed_before_gate_acquisition() {
        // Arrange
        let midge = Midge::new_in_memory("postgres").expect("create Midge");
        midge
            .create_collection(
                "scalar_reindex_removed_index",
                super::super::Schema {
                    fields: vec![super::super::FieldSchema {
                        name: "value".to_string(),
                        data_type: DataType::Text,
                        nullable: true,
                    }],
                },
            )
            .expect("create collection");
        midge
            .put_document(
                "scalar_reindex_removed_index",
                Some("row-1".to_string()),
                serde_json::json!({"value": "alpha"}),
            )
            .expect("seed collection row");
        let declared_index = IndexMeta {
            definition_semantics_version: 0,
            collection: "scalar_reindex_removed_index".to_string(),
            name: "scalar_reindex_removed_index_value".to_string(),
            field: "value".to_string(),
            fields: vec!["value".to_string()],
            expressions: Vec::new(),
            include_fields: Vec::new(),
            predicate: None,
            kind: IndexKind::Scalar,
            unique: false,
            options: std::collections::BTreeMap::new(),
        };
        midge
            .put_index(&declared_index)
            .expect("publish scalar index");
        let stale_index = midge
            .get_index(
                "scalar_reindex_removed_index",
                "scalar_reindex_removed_index_value",
            )
            .expect("read scalar index")
            .expect("published scalar index");
        let (relation_id, index_id) = Midge::scalar_index_storage_ids(&stale_index)
            .expect("read scalar index storage identity");
        let index_prefix = Midge::scalar_index_data_prefix(relation_id, index_id);
        midge
            .delete_index(
                "scalar_reindex_removed_index",
                "scalar_reindex_removed_index_value",
            )
            .expect("drop the index after the caller has read its metadata");

        // Act
        midge
            .rebuild_scalar_index_for_index(&stale_index)
            .expect("rebuild after the index has been dropped");
        let remaining_entries = midge
            .raw_scan_prefix_for_collection("scalar_reindex_removed_index", &index_prefix)
            .expect("inspect the dropped index's storage prefix");
        let remaining_metadata = midge
            .get_index(
                "scalar_reindex_removed_index",
                "scalar_reindex_removed_index_value",
            )
            .expect("check index catalog after the drop");

        // Assert
        assert!(remaining_metadata.is_none());
        assert_eq!(remaining_entries, Vec::new());
    }
}
