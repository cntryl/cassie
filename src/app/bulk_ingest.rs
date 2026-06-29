use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use super::{Cassie, CassieSession, CassieError, Uuid, TransactionRowChange};
use crate::catalog::FieldMeta;
use crate::midge::adapter::{DocumentWriteBatchOptions, DocumentWriteOp};
use crate::sql::ast::{CopyFormat, CopyStatement};
use crate::types::DataType;

const COPY_NULL: &str = "\\N";

impl Cassie {
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn copy_from_csv_stdin(
        &self,
        session: &CassieSession,
        statement: &CopyStatement,
        payload: &[u8],
    ) -> Result<usize, CassieError> {
        if session.is_transaction_active() {
            return Err(CassieError::Unsupported(
                "COPY inside an active transaction is not supported".to_string(),
            ));
        }
        if statement.format != CopyFormat::Csv {
            return Err(CassieError::Unsupported(
                "COPY only supports FORMAT csv".to_string(),
            ));
        }
        if self.catalog.is_materialized_projection(&statement.table)
            || self
                .catalog
                .materialized_projection_for_output(&statement.table)
                .is_some()
        {
            return Err(CassieError::Unsupported(format!(
                "materialized projection '{}' is read-only",
                statement.table
            )));
        }

        let schema = self
            .catalog
            .get_schema(&statement.table)
            .ok_or_else(|| CassieError::CollectionNotFound(statement.table.clone()))?;
        let columns = copy_columns(statement, schema.fields.as_slice())?;
        let mut rows = parse_csv_payload(payload)?;
        if statement.header && !rows.is_empty() {
            rows.remove(0);
        }

        let staging = CassieSession::new(session.user.clone(), session.database.clone());
        staging.begin_transaction(None)?;
        let mut seen_ids = BTreeSet::new();
        let mut affected = 0usize;

        for (row_index, row) in rows.into_iter().enumerate() {
            if row.len() != columns.len() {
                return Err(CassieError::Parse(format!(
                    "COPY row {} has {} columns but target expects {}",
                    row_index + 1,
                    row.len(),
                    columns.len()
                )));
            }

            let mut row_id = None;
            let mut payload = serde_json::Map::new();
            for (column, value) in columns.iter().zip(row) {
                match column {
                    CopyColumn::RowId => {
                        let Some(value) = value else {
                            return Err(CassieError::Parse(format!(
                                "COPY row {} has NULL row id",
                                row_index + 1
                            )));
                        };
                        row_id = Some(value);
                    }
                    CopyColumn::Field(field) => {
                        payload.insert(field.name.clone(), copy_value_to_json(value, field)?);
                    }
                }
            }

            let row_id = row_id.unwrap_or_else(|| Uuid::new_v4().to_string());
            if !seen_ids.insert(row_id.clone()) {
                return Err(CassieError::Parse(format!(
                    "COPY row {} duplicates row id '{}'",
                    row_index + 1,
                    row_id
                )));
            }

            let prepared = self.prepare_document_write_for_session(
                Some(&staging),
                &statement.table,
                serde_json::Value::Object(payload),
                true,
                None,
            )?;
            staging.stage_document_write(&statement.table, row_id, prepared);
            affected = affected.saturating_add(1);
        }

        let writes = staging
            .transaction_writes()
            .remove(&statement.table)
            .unwrap_or_default();
        if writes.is_empty() {
            return Ok(0);
        }

        let operations = copy_write_operations(writes)?;
        let report = self.midge.apply_document_write_batch_with_options(
            &statement.table,
            operations,
            DocumentWriteBatchOptions::buffered(),
        )?;
        self.runtime
            .record_projection_write_batch(statement.table.clone(), &report.stats);
        self.refresh_cardinality_stats(&statement.table)?;
        self.refresh_projection_metadata(&statement.table)?;

        let controls = self.runtime.query_controls(Instant::now());
        crate::executor::refresh_rollups_for_source_external(self, &statement.table, &controls)
            .map_err(|error| CassieError::Execution(error.to_string()))?;
        crate::executor::mark_source_projections_stale_external(self, &statement.table)
            .map_err(|error| CassieError::Execution(error.to_string()))?;

        self.midge.flush_data_family()?;
        self.runtime.bump_data_epoch();
        Ok(affected)
    }
}

#[derive(Debug, Clone)]
enum CopyColumn {
    RowId,
    Field(FieldMeta),
}

fn copy_columns(
    statement: &CopyStatement,
    fields: &[FieldMeta],
) -> Result<Vec<CopyColumn>, CassieError> {
    if statement.columns.is_empty() {
        return Ok(fields.iter().cloned().map(CopyColumn::Field).collect());
    }

    let fields_by_name = fields
        .iter()
        .map(|field| (field.name.to_ascii_lowercase(), field.clone()))
        .collect::<BTreeMap<_, _>>();
    let has_id_field = fields_by_name.contains_key("id");
    let mut out = Vec::with_capacity(statement.columns.len());

    for column in &statement.columns {
        let normalized = column.to_ascii_lowercase();
        if normalized == "_id" || (normalized == "id" && !has_id_field) {
            out.push(CopyColumn::RowId);
            continue;
        }
        let field = fields_by_name.get(&normalized).ok_or_else(|| {
            CassieError::Planner(format!(
                "COPY target column '{column}' does not exist in '{}'",
                statement.table
            ))
        })?;
        out.push(CopyColumn::Field(field.clone()));
    }

    Ok(out)
}

fn copy_write_operations(
    writes: BTreeMap<String, TransactionRowChange>,
) -> Result<Vec<DocumentWriteOp>, CassieError> {
    writes
        .into_iter()
        .map(|(id, change)| match change {
            TransactionRowChange::Upsert(payload) => Ok(DocumentWriteOp::Put { id, payload }),
            TransactionRowChange::Delete => Err(CassieError::Execution(
                "COPY staging produced a delete".to_string(),
            )),
        })
        .collect()
}

fn copy_value_to_json(
    value: Option<String>,
    field: &FieldMeta,
) -> Result<serde_json::Value, CassieError> {
    let Some(value) = value else {
        return Ok(serde_json::Value::Null);
    };

    match &field.data_type {
        DataType::SmallInt | DataType::Int | DataType::BigInt => value
            .parse::<i64>()
            .map(serde_json::Value::from)
            .map_err(|_| CassieError::Parse(format!("field '{}' expects integer", field.name))),
        DataType::Float => {
            let parsed = value
                .parse::<f64>()
                .map_err(|_| CassieError::Parse(format!("field '{}' expects float", field.name)))?;
            serde_json::Number::from_f64(parsed)
                .map(serde_json::Value::Number)
                .ok_or_else(|| {
                    CassieError::Parse(format!("field '{}' expects finite float", field.name))
                })
        }
        DataType::Boolean => parse_copy_bool(&value)
            .map(serde_json::Value::Bool)
            .ok_or_else(|| CassieError::Parse(format!("field '{}' expects boolean", field.name))),
        DataType::Json | DataType::Array(_) | DataType::Vector(_) => serde_json::from_str(&value)
            .map_err(|error| {
                CassieError::Parse(format!("field '{}' expects JSON: {error}", field.name))
            }),
        DataType::Null | DataType::Text
        | DataType::Char { .. }
        | DataType::Varchar { .. }
        | DataType::Uuid
        | DataType::Bytea
        | DataType::Date
        | DataType::Time
        | DataType::Timestamp => Ok(serde_json::Value::String(value)),
    }
}

fn parse_copy_bool(raw: &str) -> Option<bool> {
    match raw.to_ascii_lowercase().as_str() {
        "true" | "t" | "1" | "yes" | "on" => Some(true),
        "false" | "f" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn parse_csv_payload(payload: &[u8]) -> Result<Vec<Vec<Option<String>>>, CassieError> {
    let text = std::str::from_utf8(payload)
        .map_err(|_| CassieError::Parse("COPY payload must be UTF-8".to_string()))?;
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut field_started = false;
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '"' if in_quotes && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => {
                in_quotes = !in_quotes;
                field_started = true;
            }
            ',' if !in_quotes => push_csv_field(&mut row, &mut field, &mut field_started),
            '\n' if !in_quotes => push_csv_row(&mut rows, &mut row, &mut field, &mut field_started),
            '\r' if !in_quotes => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                push_csv_row(&mut rows, &mut row, &mut field, &mut field_started);
            }
            _ => {
                field.push(ch);
                field_started = true;
            }
        }
    }

    if in_quotes {
        return Err(CassieError::Parse(
            "COPY CSV payload has unterminated quote".into(),
        ));
    }
    if field_started || !row.is_empty() {
        push_csv_row(&mut rows, &mut row, &mut field, &mut field_started);
    }

    Ok(rows)
}

fn push_csv_row(
    rows: &mut Vec<Vec<Option<String>>>,
    row: &mut Vec<Option<String>>,
    field: &mut String,
    field_started: &mut bool,
) {
    push_csv_field(row, field, field_started);
    rows.push(std::mem::take(row));
}

fn push_csv_field(row: &mut Vec<Option<String>>, field: &mut String, field_started: &mut bool) {
    let value = std::mem::take(field);
    row.push((value != COPY_NULL).then_some(value));
    *field_started = false;
}
