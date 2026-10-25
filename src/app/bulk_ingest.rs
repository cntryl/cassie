use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use super::{Cassie, CassieError, CassieSession, TransactionRowChange, Uuid};
use crate::catalog::FieldMeta;
use crate::midge::adapter::DocumentWriteOp;
use crate::runtime::QueryCancellationHandle;
use crate::sql::ast::{CopyFormat, CopyStatement};
use crate::types::DataType;

const COPY_NULL: &str = "\\N";

#[derive(Clone, Copy)]
pub(crate) struct CopyCsvOptions<'a> {
    pub(crate) delimiter: char,
    pub(crate) quote: char,
    pub(crate) escape: char,
    pub(crate) null_marker: &'a str,
    pub(crate) force_null: &'a [usize],
    pub(crate) force_not_null: &'a [usize],
}

#[derive(Clone, Copy)]
pub(crate) struct CopyIngestOptions<'a> {
    pub(crate) default_marker: Option<&'a str>,
    pub(crate) on_error_ignore: bool,
    pub(crate) reject_limit: Option<usize>,
}

#[derive(Debug)]
pub(crate) struct CopySkippedRow {
    pub(crate) line: usize,
    pub(crate) column: String,
    pub(crate) value: String,
    error: CassieError,
}

#[derive(Debug)]
pub(crate) struct CopyIngestResult {
    pub(crate) count: usize,
    pub(crate) skipped: Vec<CopySkippedRow>,
}

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
        let transactional = session.is_transaction_active();
        let result = self.copy_from_csv_stdin_inner(session, statement, payload, None);
        if result.is_err() && transactional {
            session.mark_transaction_failed();
        }
        result
    }

    /// Copies CSV rows atomically while honoring an external query-cancellation handle.
    ///
    /// # Errors
    ///
    /// Returns an error when cancellation, validation, storage, or execution fails.
    pub fn copy_from_csv_stdin_with_cancellation(
        &self,
        session: &CassieSession,
        statement: &CopyStatement,
        payload: &[u8],
        cancellation: &QueryCancellationHandle,
    ) -> Result<usize, CassieError> {
        let transactional = session.is_transaction_active();
        let result =
            self.copy_from_csv_stdin_inner(session, statement, payload, Some(cancellation));
        if result.is_err() && transactional {
            session.mark_transaction_failed();
        }
        result
    }

    fn copy_from_csv_stdin_inner(
        &self,
        session: &CassieSession,
        statement: &CopyStatement,
        payload: &[u8],
        cancellation: Option<&QueryCancellationHandle>,
    ) -> Result<usize, CassieError> {
        let mut rows = parse_csv_payload(payload)?;
        if statement.header && !rows.is_empty() {
            rows.remove(0);
        }
        self.copy_from_rows_inner(
            session,
            statement,
            rows,
            cancellation,
            CopyIngestOptions {
                default_marker: None,
                on_error_ignore: false,
                reject_limit: None,
            },
        )
        .map(|result| result.count)
    }

    pub(crate) fn ingest_copy_rows_with_default_marker(
        &self,
        session: &CassieSession,
        statement: &CopyStatement,
        rows: Vec<Vec<Option<String>>>,
        cancellation: Option<&QueryCancellationHandle>,
        options: CopyIngestOptions<'_>,
    ) -> Result<CopyIngestResult, CassieError> {
        let transactional = session.is_transaction_active();
        let result = self.copy_from_rows_inner(session, statement, rows, cancellation, options);
        if result.is_err() && transactional {
            session.mark_transaction_failed();
        }
        result
    }

    fn copy_from_rows_inner(
        &self,
        session: &CassieSession,
        statement: &CopyStatement,
        rows: Vec<Vec<Option<String>>>,
        cancellation: Option<&QueryCancellationHandle>,
        options: CopyIngestOptions<'_>,
    ) -> Result<CopyIngestResult, CassieError> {
        check_copy_cancellation(cancellation)?;
        let database = session.current_database().unwrap_or(&self.default_database);
        let context =
            crate::sql::binder::BindingContext::scoped(database, self.binding_search_path(session));
        let mut statement = statement.clone();
        if let Some(relation_database) = crate::catalog::relation_database_name(&statement.table) {
            if !relation_database.eq_ignore_ascii_case(database) {
                return Err(CassieError::Unsupported(
                    "cross-database relation references are not supported".to_string(),
                ));
            }
        } else {
            statement.table =
                crate::sql::binder::normalize_relation_name(&statement.table, &context)?;
        }
        validate_copy_request(self, session, &statement)?;

        let schema = self
            .catalog
            .get_schema(&statement.table)
            .ok_or_else(|| CassieError::CollectionNotFound(statement.table.clone()))?;
        let columns = copy_columns(&statement, schema.fields.as_slice())?;

        let collections = self.referential_write_collections(&statement.table);
        let (affected, stats, skipped) =
            self.midge.with_collection_write_gates(&collections, || {
                self.stage_and_apply_copy_rows(
                    session,
                    &statement,
                    &columns,
                    rows,
                    cancellation,
                    options,
                )
            })?;
        let Some(stats) = stats else {
            return Ok(CopyIngestResult {
                count: affected,
                skipped,
            });
        };
        finish_copy_batch(self, &statement, &stats)?;
        self.runtime.set_data_epoch(self.midge.data_epoch()?);
        Ok(CopyIngestResult {
            count: affected,
            skipped,
        })
    }

    /// Prepares, validates and applies COPY rows while the caller holds the
    /// referential write gates for the target table.
    fn stage_and_apply_copy_rows(
        &self,
        session: &CassieSession,
        statement: &CopyStatement,
        columns: &[CopyColumn],
        rows: Vec<Vec<Option<String>>>,
        cancellation: Option<&QueryCancellationHandle>,
        options: CopyIngestOptions<'_>,
    ) -> Result<
        (
            usize,
            Option<crate::runtime::ProjectionWriteStats>,
            Vec<CopySkippedRow>,
        ),
        CassieError,
    > {
        let transactional = session.is_transaction_active();
        let batch = session.fork_statement_batch()?;
        let staging = batch.session();
        let mut references = super::foreign_key_checks::ForeignKeyReferences::default();
        let schema = self.catalog.get_schema(&statement.table);
        let mut seen_ids = BTreeSet::new();
        let mut affected = 0usize;
        let mut skipped_rows = 0usize;
        let mut skipped = Vec::new();

        for (row_index, row) in rows.into_iter().enumerate() {
            check_copy_cancellation(cancellation)?;
            let input = match convert_copy_row(columns, row, row_index + 1, options)? {
                CopyRowOutcome::Accepted(input) => input,
                CopyRowOutcome::Skipped(rejected) => {
                    skipped_rows = skipped_rows.saturating_add(1);
                    if options
                        .reject_limit
                        .is_some_and(|limit| skipped_rows > limit)
                    {
                        return Err(rejected.error);
                    }
                    skipped.push(rejected);
                    continue;
                }
            };
            let row_id = input.row_id.unwrap_or_else(|| Uuid::new_v4().to_string());
            if !seen_ids.insert(row_id.clone()) {
                return Err(CassieError::Parse(format!(
                    "COPY row {} duplicates row id '{}'",
                    row_index + 1,
                    row_id
                )));
            }

            let payload = self.apply_insert_defaults(
                &statement.table,
                serde_json::Value::Object(input.payload),
                &input.explicit_json_nulls,
            )?;
            let (prepared, constraints) = self.prepare_document_write_deferring_foreign_keys(
                Some(staging),
                &statement.table,
                payload,
                false,
                None,
            )?;
            references.collect(&statement.table, &constraints, &prepared, schema.as_ref())?;
            staging.stage_document_write(&statement.table, row_id, prepared)?;
            affected = affected.saturating_add(1);
        }

        check_copy_cancellation(cancellation)?;
        self.validate_foreign_key_references(Some(staging), &statement.table, &references)?;

        if transactional {
            check_copy_cancellation(cancellation)?;
            session.publish_statement_batch(&batch)?;
            return Ok((affected, None, skipped));
        }

        // Autocommit COPY has no COMMIT, so run its parent-side check here: an
        // explicit `_id` can replace a referenced key that children still use.
        self.validate_staged_parent_references(staging)?;
        let writes = staging
            .transaction_writes()
            .remove(&statement.table)
            .unwrap_or_default();
        if writes.is_empty() {
            return Ok((0, None, skipped));
        }

        let operations = copy_write_operations(writes)?;
        check_copy_cancellation(cancellation)?;
        let options = self.buffered_document_write_options(&statement.table);
        let report = self.midge.apply_document_write_batch_with_options(
            &statement.table,
            operations,
            &options,
        )?;
        Ok((affected, Some(report.stats), skipped))
    }
}

struct CopyInputRow<'a> {
    row_id: Option<String>,
    payload: serde_json::Map<String, serde_json::Value>,
    explicit_json_nulls: Vec<&'a str>,
}

enum CopyRowOutcome<'a> {
    Accepted(CopyInputRow<'a>),
    Skipped(CopySkippedRow),
}

fn convert_copy_row<'a>(
    columns: &'a [CopyColumn],
    row: Vec<Option<String>>,
    line: usize,
    options: CopyIngestOptions<'_>,
) -> Result<CopyRowOutcome<'a>, CassieError> {
    if row.len() != columns.len() {
        return Err(super::DiagnosticError::new(
            "22P04",
            format!(
                "COPY row {line} has {} columns but target expects {}",
                row.len(),
                columns.len()
            ),
        )
        .into());
    }

    let mut row_id = None;
    let mut payload = serde_json::Map::new();
    let mut explicit_json_nulls = Vec::new();
    for (column, value) in columns.iter().zip(row) {
        match column {
            CopyColumn::RowId => {
                row_id = Some(value.ok_or_else(|| {
                    CassieError::Parse(format!("COPY row {line} has NULL row id"))
                })?);
            }
            CopyColumn::Field(field) => {
                if options
                    .default_marker
                    .is_some_and(|marker| value.as_deref() == Some(marker))
                {
                    continue;
                }
                if value.is_none() && matches!(field.data_type, DataType::Json | DataType::Jsonb) {
                    explicit_json_nulls.push(field.name.as_str());
                    continue;
                }
                let source_value = value.clone();
                match copy_value_to_json(value, field) {
                    Ok(value) => {
                        payload.insert(field.name.clone(), value);
                    }
                    Err(error) if options.on_error_ignore && is_copy_input_error(&error) => {
                        return Ok(CopyRowOutcome::Skipped(CopySkippedRow {
                            line,
                            column: field.name.clone(),
                            value: source_value.unwrap_or_default(),
                            error,
                        }));
                    }
                    Err(error) => return Err(error),
                }
            }
        }
    }
    Ok(CopyRowOutcome::Accepted(CopyInputRow {
        row_id,
        payload,
        explicit_json_nulls,
    }))
}

fn is_copy_input_error(error: &CassieError) -> bool {
    error.descriptor().sql_state.starts_with("22")
}

fn check_copy_cancellation(
    cancellation: Option<&QueryCancellationHandle>,
) -> Result<(), CassieError> {
    if cancellation.is_some_and(QueryCancellationHandle::is_cancelled) {
        Err(CassieError::QueryCancelled)
    } else {
        Ok(())
    }
}

fn validate_copy_request(
    cassie: &Cassie,
    session: &CassieSession,
    statement: &CopyStatement,
) -> Result<(), CassieError> {
    if session.is_transaction_failed() {
        return Err(CassieError::failed_transaction());
    }
    if statement.format != CopyFormat::Csv {
        return Err(CassieError::Unsupported(
            "COPY only supports FORMAT csv".to_string(),
        ));
    }
    if cassie.catalog.is_materialized_projection(&statement.table)
        || cassie
            .catalog
            .materialized_projection_for_output(&statement.table)
            .is_some()
    {
        return Err(CassieError::Unsupported(format!(
            "materialized projection '{}' is read-only",
            statement.table
        )));
    }
    Ok(())
}

fn finish_copy_batch(
    cassie: &Cassie,
    statement: &CopyStatement,
    stats: &crate::runtime::ProjectionWriteStats,
) -> Result<(), CassieError> {
    cassie
        .runtime
        .record_projection_write_batch(statement.table.clone(), stats);
    cassie.refresh_cardinality_stats(&statement.table)?;
    cassie.refresh_projection_metadata(&statement.table)?;

    let controls = cassie.runtime.query_controls(Instant::now());
    crate::executor::refresh_rollups_for_source_external(cassie, &statement.table, &controls)
        .map_err(|error| CassieError::Execution(error.to_string()))?;
    crate::executor::mark_source_projections_stale_external(cassie, &statement.table)
        .map_err(|error| CassieError::Execution(error.to_string()))?;

    cassie.midge.flush_data_family()?;
    Ok(())
}

#[derive(Debug, Clone)]
pub(crate) enum CopyColumn {
    RowId,
    Field(FieldMeta),
}

pub(crate) fn copy_columns(
    statement: &CopyStatement,
    fields: &[FieldMeta],
) -> Result<Vec<CopyColumn>, CassieError> {
    if statement.columns.is_empty() {
        return Ok(fields.iter().cloned().map(CopyColumn::Field).collect());
    }

    let has_id_field =
        crate::types::row_identity::declares_id(fields.iter().map(|field| field.name.as_str()));
    let mut out = Vec::with_capacity(statement.columns.len());

    for column in &statement.columns {
        if crate::types::row_identity::is_identity_reference(column, has_id_field) {
            out.push(CopyColumn::RowId);
            continue;
        }
        let field = fields
            .iter()
            .find(|field| field.name == *column)
            .or_else(|| {
                fields.iter().find(|field| {
                    crate::sql::ColumnIdentifierPath::matches_stored_field(column, &field.name)
                })
            })
            .ok_or_else(|| {
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
        DataType::SmallInt | DataType::Int | DataType::BigInt => {
            copy_integer_value(&value, &field.data_type)
        }
        DataType::Float => {
            let parsed = crate::types::float_input::parse(&value)?;
            serde_json::Number::from_f64(parsed)
                .map(serde_json::Value::Number)
                .ok_or(CassieError::FloatInput(
                    crate::types::FloatInputError::Range,
                ))
        }
        DataType::ObjectId(_) => crate::types::object_id::parse_oid_text(&value)
            .map(serde_json::Value::from)
            .map_err(CassieError::from),
        DataType::Extended(kind) => kind
            .parse_text(&value)
            .map(|typed| typed.to_json())
            .map_err(CassieError::from),
        DataType::Boolean => parse_copy_bool(&value)
            .map(serde_json::Value::Bool)
            .ok_or_else(|| crate::types::BooleanInputError.into()),
        DataType::Array(element_type) => {
            crate::types::array::parse_text_array(&value, element_type).map_err(|error| {
                let message = format!("field '{}' expects array: {error}", field.name);
                match error {
                    crate::types::array::TextArrayError::Invalid(_) => CassieError::Parse(message),
                    crate::types::array::TextArrayError::UnsupportedJsonDocumentNull => {
                        CassieError::Unsupported(message)
                    }
                }
            })
        }
        DataType::Json | DataType::Jsonb | DataType::Vector(_) => serde_json::from_str(&value)
            .map_err(|error| {
                super::DiagnosticError::new(
                    "22P02",
                    format!("field '{}' expects JSON: {error}", field.name),
                )
                .into()
            }),
        DataType::Name => Ok(serde_json::Value::String(crate::types::name::parse_text(
            &value,
        ))),
        DataType::Null
        | DataType::JsonPath
        | DataType::Text
        | DataType::Char { .. }
        | DataType::Varchar { .. }
        | DataType::Uuid
        | DataType::Bytea
        | DataType::Date
        | DataType::Time
        | DataType::Timestamp => Ok(serde_json::Value::String(value)),
    }
}

fn copy_integer_value(value: &str, data_type: &DataType) -> Result<serde_json::Value, CassieError> {
    use crate::types::integer::{IntegerInputError, IntegerWidth};
    let width = IntegerWidth::from_data_type(data_type).expect("integer COPY field");
    crate::types::integer::parse(value, width)
        .map(serde_json::Value::from)
        .map_err(|error| {
            let (state, message) = match error {
                IntegerInputError::Syntax => ("22P02", width.syntax_message()),
                IntegerInputError::Range => ("22003", width.range_message()),
            };
            super::DiagnosticError::new(state, message).into()
        })
}

fn parse_copy_bool(raw: &str) -> Option<bool> {
    crate::types::boolean::parse_text(raw)
}

pub(crate) fn parse_copy_csv_payload_with_force(
    payload: &[u8],
    options: CopyCsvOptions<'_>,
) -> Result<Vec<Vec<Option<String>>>, CassieError> {
    parse_csv_payload_with_null_policy(payload, options, options.null_marker.is_empty())
}

fn parse_csv_payload_with_null_policy(
    payload: &[u8],
    options: CopyCsvOptions<'_>,
    empty_unquoted_is_null: bool,
) -> Result<Vec<Vec<Option<String>>>, CassieError> {
    let text = std::str::from_utf8(payload)
        .map_err(|_| CassieError::Parse("COPY payload must be UTF-8".to_string()))?;
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = CsvField::default();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            ch if in_quotes && ch == options.escape && chars.peek() == Some(&options.quote) => {
                field.value.push(options.quote);
                chars.next();
            }
            ch if ch == options.quote => {
                in_quotes = !in_quotes;
                field.started = true;
                field.quoted = true;
            }
            ch if ch == options.delimiter && !in_quotes => {
                field.push_into(
                    &mut row,
                    options.null_marker,
                    empty_unquoted_is_null,
                    options.force_null,
                    options.force_not_null,
                );
            }
            '\n' if !in_quotes => {
                push_csv_row(
                    &mut rows,
                    &mut row,
                    &mut field,
                    options.null_marker,
                    empty_unquoted_is_null,
                    options.force_null,
                    options.force_not_null,
                );
            }
            '\r' if !in_quotes => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                push_csv_row(
                    &mut rows,
                    &mut row,
                    &mut field,
                    options.null_marker,
                    empty_unquoted_is_null,
                    options.force_null,
                    options.force_not_null,
                );
            }
            _ => {
                field.value.push(ch);
                field.started = true;
            }
        }
    }

    if in_quotes {
        return Err(super::DiagnosticError::new(
            "22P04",
            "COPY CSV payload has unterminated quote",
        )
        .into());
    }
    if field.started || !row.is_empty() {
        push_csv_row(
            &mut rows,
            &mut row,
            &mut field,
            options.null_marker,
            empty_unquoted_is_null,
            options.force_null,
            options.force_not_null,
        );
    }

    Ok(rows)
}

fn push_csv_row(
    rows: &mut Vec<Vec<Option<String>>>,
    row: &mut Vec<Option<String>>,
    field: &mut CsvField,
    null_marker: &str,
    empty_unquoted_is_null: bool,
    force_null: &[usize],
    force_not_null: &[usize],
) {
    field.push_into(
        row,
        null_marker,
        empty_unquoted_is_null,
        force_null,
        force_not_null,
    );
    rows.push(std::mem::take(row));
}

#[derive(Default)]
struct CsvField {
    value: String,
    started: bool,
    quoted: bool,
}

impl CsvField {
    /// PostgreSQL CSV treats only an unquoted null marker as NULL.
    fn push_into(
        &mut self,
        row: &mut Vec<Option<String>>,
        null_marker: &str,
        empty_unquoted_is_null: bool,
        force_null: &[usize],
        force_not_null: &[usize],
    ) {
        let field = std::mem::take(self);
        let index = row.len();
        let marker_match = field.value == null_marker;
        let empty_match = empty_unquoted_is_null && field.value.is_empty() && !field.quoted;
        let is_null = if field.quoted {
            marker_match && force_null.contains(&index)
        } else {
            (marker_match || empty_match) && !force_not_null.contains(&index)
        };
        row.push((!is_null).then_some(field.value));
    }
}

fn parse_csv_payload(payload: &[u8]) -> Result<Vec<Vec<Option<String>>>, CassieError> {
    parse_csv_payload_with_null_policy(
        payload,
        CopyCsvOptions {
            delimiter: ',',
            quote: '"',
            escape: '"',
            null_marker: COPY_NULL,
            force_null: &[],
            force_not_null: &[],
        },
        true,
    )
}

#[cfg(test)]
mod tests {
    use super::parse_csv_payload;

    #[test]
    fn should_parse_copy_csv_quoted_newlines() {
        // Arrange
        let payload = b"row-1,\"first line\nsecond line\"\n";

        // Act
        let rows = parse_csv_payload(payload).expect("quoted newline");

        // Assert
        assert_eq!(
            rows,
            vec![vec![
                Some("row-1".to_string()),
                Some("first line\nsecond line".to_string())
            ]]
        );
    }

    #[test]
    fn should_parse_copy_csv_escaped_quotes() {
        // Arrange
        let payload = b"row-1,\"a \"\"quoted\"\" value\"\n";

        // Act
        let rows = parse_csv_payload(payload).expect("escaped quotes");

        // Assert
        assert_eq!(rows[0][1], Some("a \"quoted\" value".to_string()));
    }

    #[test]
    fn should_treat_quoted_copy_null_marker_as_literal() {
        // Arrange
        let payload = b"row-1,\\N,\"\\N\"\n";

        // Act
        let rows = parse_csv_payload(payload).expect("null markers");

        // Assert
        assert_eq!(
            rows[0],
            vec![Some("row-1".to_string()), None, Some("\\N".to_string())]
        );
    }

    #[test]
    fn should_parse_unquoted_empty_csv_fields_as_null_in_every_position() {
        // Arrange
        let payload = b",\"\",\r\n\"q\",,";

        // Act
        let rows = parse_csv_payload(payload).expect("empty fields");

        // Assert
        assert_eq!(
            rows,
            vec![
                vec![None, Some(String::new()), None],
                vec![Some("q".to_string()), None, None],
            ]
        );
    }
}
