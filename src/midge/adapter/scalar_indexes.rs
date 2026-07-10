use super::{
    check_document_write_failure_point, key_encoding, CassieError, DocumentWriteFailurePoint,
    Midge, RowDecode,
};
use crate::catalog::{IndexKind, IndexMeta};
use crate::executor::filter;
use crate::sql::ast::Expr;
use crate::types::Value;
use cntryl_midge::{Query, WriteOptions};
use std::collections::{BTreeMap, HashMap};

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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
struct ScalarIndexStoredRow {
    id: String,
    fields: BTreeMap<String, serde_json::Value>,
}

type ScalarIndexEntry = (Vec<u8>, Vec<u8>);

impl Midge {
    pub(crate) fn sync_scalar_indexes_for_document(
        tx: &mut cntryl_midge::Transaction,
        id: &str,
        old_payload: Option<&serde_json::Value>,
        new_payload: Option<&serde_json::Value>,
        indexes: &[IndexMeta],
    ) -> Result<(usize, usize), CassieError> {
        let mut deletes = 0usize;
        let mut puts = 0usize;

        for index in indexes {
            let old_entry = match old_payload {
                Some(payload) => Self::scalar_index_entry(index, id, payload)?,
                None => None,
            };
            let new_entry = match new_payload {
                Some(payload) => Self::scalar_index_entry(index, id, payload)?,
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
        let mut tx = self.begin_data_rw_tx()?;
        Self::delete_keys_with_prefix(
            &mut tx,
            Self::scalar_index_data_prefix(&index.collection, &index.name),
        )?;

        for row in rows {
            if let Some((key, value)) = Self::scalar_index_entry(index, &row.id, &row.payload)? {
                tx.put(key, value, None).map_err(CassieError::from)?;
            }
        }

        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(())
    }

    pub(crate) fn delete_scalar_index_data(
        &self,
        collection: &str,
        index_name: &str,
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_data_rw_tx()?;
        Self::delete_keys_with_prefix(
            &mut tx,
            Self::scalar_index_data_prefix(collection, index_name),
        )?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(())
    }

    pub(crate) fn scan_scalar_index(
        &self,
        index: &IndexMeta,
        request: &ScalarIndexScanRequest,
    ) -> Result<Vec<ScalarIndexScanHit>, CassieError> {
        if !Self::scalar_index_supports_storage(index) {
            return Err(CassieError::Unsupported(format!(
                "scalar index '{}' is not storage-backed",
                index.name
            )));
        }

        let tx = self.begin_data_readonly_tx()?;
        let data_prefix = Self::scalar_index_data_prefix(&index.collection, &index.name);
        let seek_prefix = key_encoding::scalar_index_seek_prefix(
            &index.collection,
            &index.name,
            &request.equality_prefix,
        )?;
        let (start_key, end_key) = key_encoding::scalar_index_query_bounds(
            &seek_prefix,
            request.lower_bound.as_ref(),
            request.upper_bound.as_ref(),
        )?;
        let mut query = Query::new().prefix(data_prefix.into());
        if let Some(start_key) = start_key {
            query = query.start_key(start_key.into());
        }
        if let Some(end_key) = end_key {
            query = query.end_key(end_key.into());
        }
        if request.reverse {
            query = query.reverse();
        }

        let scan = tx.scan(&query).map_err(CassieError::from)?;
        let mut hits = Vec::new();
        let limit = request.limit.unwrap_or(usize::MAX);

        for (_key, raw_value) in scan {
            let stored: ScalarIndexStoredRow =
                serde_json::from_slice(&raw_value).map_err(|error| {
                    CassieError::Parse(format!(
                        "invalid scalar index entry for '{}': {error}",
                        index.name
                    ))
                })?;
            hits.push(ScalarIndexScanHit {
                id: stored.id,
                fields: stored.fields.into_iter().collect(),
            });
            if hits.len() >= limit {
                break;
            }
        }

        Ok(hits)
    }

    fn scalar_index_supports_storage(index: &IndexMeta) -> bool {
        index.kind == IndexKind::Scalar
            && (!index.normalized_fields().is_empty() || !index.normalized_expressions().is_empty())
    }

    fn scalar_index_entry(
        index: &IndexMeta,
        id: &str,
        payload: &serde_json::Value,
    ) -> Result<Option<ScalarIndexEntry>, CassieError> {
        if !Self::scalar_index_supports_storage(index)
            || !Self::payload_matches_scalar_index_predicate(index, payload)?
        {
            return Ok(None);
        }

        let Some(key_values) = Self::scalar_index_key_values(index, payload)? else {
            return Ok(None);
        };
        let key =
            key_encoding::scalar_index_entry_key(&index.collection, &index.name, &key_values, id)?;
        let value = serde_json::to_vec(&ScalarIndexStoredRow {
            id: id.to_string(),
            fields: Self::scalar_index_stored_fields(index, payload),
        })
        .map_err(|error| CassieError::Parse(error.to_string()))?;
        Ok(Some((key, value)))
    }

    pub(crate) fn scalar_index_key_values(
        index: &IndexMeta,
        payload: &serde_json::Value,
    ) -> Result<Option<Vec<serde_json::Value>>, CassieError> {
        let mut values = Vec::new();
        for field in index.normalized_fields() {
            let Some(value) = payload.get(&field) else {
                return Ok(None);
            };
            if value.is_null() {
                return Ok(None);
            }
            values.push(value.clone());
        }

        let expressions = index.normalized_expressions();
        if expressions.is_empty() {
            return Ok(Some(values));
        }

        let row = payload_to_row(payload);
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

    fn payload_matches_scalar_index_predicate(
        index: &IndexMeta,
        payload: &serde_json::Value,
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
        let row = payload_to_row(payload);
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

fn payload_to_row(payload: &serde_json::Value) -> Vec<(String, Value)> {
    let Some(object) = payload.as_object() else {
        return Vec::new();
    };
    object
        .iter()
        .map(|(field, value)| (field.clone(), json_to_query_value(value)))
        .collect()
}

fn json_to_query_value(value: &serde_json::Value) -> Value {
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
        Value::Vector(value) => Ok(serde_json::Value::Array(
            value
                .values
                .into_iter()
                .filter_map(|value| serde_json::Number::from_f64(f64::from(value)))
                .map(serde_json::Value::Number)
                .collect(),
        )),
        Value::Json(value) => Ok(value),
    }
}
