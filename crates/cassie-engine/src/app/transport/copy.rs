//! Engine-owned admission and publication for a decoded COPY FROM operation.

use std::sync::Arc;
use std::time::Instant;

use crate::app::{
    Cassie, CassieError, CassieSession, CopyColumn, CopyIngestOptions, CopyIngestResult,
    CopyCommandOwner, FailureAuthority,
};
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls, QueryMemoryReservation};
use crate::sql::ast::{CopyStatement, QueryStatement};

#[path = "copy/filter.rs"]
mod filter;
#[path = "copy/filter_text.rs"]
mod filter_text;

/// Header policy decoded from the server's COPY options.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CopyHeaderMode {
    None,
    Skip,
    Match,
}

/// FORCE selection decoded from the COPY option list.
pub enum CopyForceSelection {
    None,
    All,
    Columns(Vec<String>),
}

/// Semantic options applied after transport decoding.
pub struct CopyExecutionOptions<'a> {
    pub header: CopyHeaderMode,
    pub where_clause: Option<&'a str>,
    pub default_marker: Option<&'a str>,
    pub on_error_ignore: bool,
    pub reject_limit: Option<usize>,
}

/// Engine-side error from semantic COPY row processing.
pub use filter::CopyWhereError as CopyOperationError;

/// Opaque admitted operation that pins the validated COPY target and write
/// authority between COPY response framing and publication.
pub struct CopyFromOperation {
    cassie: Arc<Cassie>,
    statement: CopyStatement,
    owner: Option<CopyCommandOwner>,
    authority: FailureAuthority,
    controls: QueryExecutionControls,
    _admission: QueryMemoryReservation,
}

impl CopyFromOperation {
    /// Binds, validates, and admits a COPY FROM target before accepting rows.
    ///
    /// # Errors
    /// Returns parse, authorization, transaction, schema, deadline, or memory errors.
    pub fn bind(
        cassie: Arc<Cassie>,
        session: &CassieSession,
        binding_sql: &str,
        force_null: &CopyForceSelection,
        force_not_null: &CopyForceSelection,
        cancellation: Option<QueryCancellationHandle>,
    ) -> Result<Self, CassieError> {
        let authority = FailureAuthority::capture_for_operation(session);
        let parsed = cassie.parse_statement_for_session(Some(session), binding_sql)?;
        session.authorize_statement(&parsed.statement)?;
        cassie.authorize_role_object_access(session, &parsed.statement)?;
        Cassie::ensure_transaction_recovery(session, &parsed)?;
        session.ensure_transaction_writable()?;
        let started = Instant::now();
        let mut controls = QueryExecutionControls::with_cancellation(
            &cassie.runtime().limits(),
            started,
            cancellation.unwrap_or_default(),
        );
        session.apply_statement_timeout(&mut controls, started);
        cassie.prepare_transaction_read_view(session, &parsed, &controls)?;
        let QueryStatement::Copy(copy) = &parsed.statement else {
            return Err(CassieError::Parse(
                "COPY FROM STDIN did not parse as a table copy".into(),
            ));
        };
        let (owner, owned_context) = if session.is_transaction_active() {
            let (owner, context) =
                cassie.capture_bound_copy_command_owner(session, &copy.table, &controls)?;
            (Some(owner), Some(context))
        } else {
            (None, None)
        };
        let fallback_context = owner.is_none().then(|| {
            let database = session
                .current_database()
                .unwrap_or(cassie.default_database())
                .to_owned();
            let search_path = cassie.binding_search_path(session);
            if cassie.database_catalog_enforced() {
                crate::sql::binder::BindingContext::scoped(database, search_path)
            } else {
                crate::sql::binder::BindingContext::unscoped(database, search_path)
            }
        });
        let context = owned_context
            .as_ref()
            .map(crate::app::CopyBindingContext::context)
            .or(fallback_context.as_ref())
            .expect("COPY binding context");
        let scoped = owner.as_ref().map_or_else(
            || (*cassie).clone(),
            |owner| cassie.with_copy_command_owner(session, owner),
        );
        session.mark_transaction_query();
        let bound = crate::sql::binder::bind_with_context(parsed, &scoped.catalog, context)?;
        let QueryStatement::Copy(statement) = bound.statement.statement else {
            return Err(CassieError::Parse(
                "COPY FROM STDIN did not bind as a table copy".into(),
            ));
        };
        if owner
            .as_ref()
            .is_some_and(|owner| !owner.matches_target(&statement.table))
        {
            return Err(CassieError::Execution(
                "COPY target changed during binding".into(),
            ));
        }
        let schema = scoped
            .catalog
            .get_schema(&statement.table)
            .ok_or_else(|| CassieError::Parse("COPY target schema disappeared".into()))?;
        validate_force_columns(&statement, &schema.fields, force_null, "FORCE_NULL")?;
        validate_force_columns(
            &statement,
            &schema.fields,
            force_not_null,
            "FORCE_NOT_NULL",
        )?;
        authority.reserve_shared_owner(&controls)?;
        let owner_bytes = if owner.is_some() {
            std::mem::size_of::<CopyCommandOwner>()
        } else {
            0
        };
        let admission_bytes = std::mem::size_of::<Self>()
            .checked_sub(owner_bytes)
            .ok_or_else(|| CassieError::ResourceLimit("COPY binding size underflow".into()))?;
        controls.check_interruption()?;
        let admission = controls.reserve_query_memory(admission_bytes)?;
        Ok(Self {
            cassie: Arc::new(scoped),
            statement,
            owner,
            authority,
            controls,
            _admission: admission,
        })
    }

    /// Request controls associated with the operation, borrowed for decoding.
    #[must_use]
    pub fn controls(&self) -> &QueryExecutionControls {
        &self.controls
    }

    /// Bound target statement used by the server's wire decoder.
    #[must_use]
    pub fn statement(&self) -> &CopyStatement {
        &self.statement
    }

    /// Typed target columns used by transport format decoders.
    #[must_use]
    pub fn target_columns(&self) -> Result<Vec<CopyColumn>, CassieError> {
        let schema = self
            .cassie
            .catalog
            .get_schema(&self.statement.table)
            .ok_or_else(|| CassieError::Parse("COPY target schema disappeared".into()))?;
        crate::app::copy_columns(&self.statement, &schema.fields)
    }

    /// Number of columns in the PostgreSQL COPY response.
    #[must_use]
    pub fn copy_response_column_count(&self) -> usize {
        if self.statement.columns.is_empty() {
            self.cassie
                .catalog
                .get_schema(&self.statement.table)
                .map_or(0, |schema| schema.fields.len())
        } else {
            self.statement.columns.len()
        }
    }

    /// Column positions forced to null by CSV decoding.
    #[must_use]
    pub fn force_null_indices(&self, selection: &CopyForceSelection) -> Result<Vec<usize>, CassieError> {
        Ok(force_indices(selection, &self.target_columns()?))
    }

    /// Column positions forced to non-null by CSV decoding.
    #[must_use]
    pub fn force_not_null_indices(&self, selection: &CopyForceSelection) -> Result<Vec<usize>, CassieError> {
        Ok(force_indices(selection, &self.target_columns()?))
    }

    /// Validates and publishes decoded rows under the retained owner authority.
    ///
    /// # Errors
    /// Returns row format, stale owner, WHERE, or publication errors.
    pub fn execute_decoded_rows(
        &self,
        session: &CassieSession,
        mut rows: Vec<Vec<Option<String>>>,
        options: CopyExecutionOptions<'_>,
        cancellation: Option<&QueryCancellationHandle>,
    ) -> Result<CopyIngestResult, CopyOperationError> {
        let columns = self.target_columns()?;
        validate_header(&columns, options.header, &rows)?;
        if options.header != CopyHeaderMode::None && !rows.is_empty() {
            rows.remove(0);
        }
        let operation_session = session.with_operation_failure_authority(Some(&self.authority));
        if let Some(owner) = &self.owner {
            owner.ensure_current_transaction(&operation_session, &self.controls)?;
        }
        let _filter_rows_memory;
        if let Some(predicate) = options.where_clause {
            let (accepted, memory) = filter::filter_rows_for_operation(
                &self.cassie,
                &operation_session,
                &self.statement,
                rows,
                predicate,
                (&self.controls, Some(&self.authority)),
            )?;
            rows = accepted;
            _filter_rows_memory = Some(memory);
        } else {
            _filter_rows_memory = None;
        }
        let result = self.cassie.ingest_copy_rows_with_default_marker(
            &operation_session,
            &self.statement,
            rows,
            cancellation,
            CopyIngestOptions {
                default_marker: options.default_marker,
                on_error_ignore: options.on_error_ignore,
                reject_limit: options.reject_limit,
            },
            self.owner.as_ref(),
        )?;
        drop(_filter_rows_memory);
        Ok(result)
    }
}

fn force_indices(selection: &CopyForceSelection, columns: &[CopyColumn]) -> Vec<usize> {
    match selection {
        CopyForceSelection::None => Vec::new(),
        CopyForceSelection::All => (0..columns.len()).collect(),
        CopyForceSelection::Columns(names) => columns
            .iter()
            .enumerate()
            .filter_map(|(index, column)| match column {
                CopyColumn::Field(field) if names.iter().any(|name| name == &field.name) => {
                    Some(index)
                }
                CopyColumn::RowId if names.iter().any(|name| name == "_id" || name == "id") => {
                    Some(index)
                }
                _ => None,
            })
            .collect(),
    }
}

fn validate_force_columns(
    statement: &CopyStatement,
    fields: &[crate::catalog::FieldMeta],
    selection: &CopyForceSelection,
    option: &str,
) -> Result<(), CassieError> {
    let CopyForceSelection::Columns(names) = selection else {
        return Ok(());
    };
    let columns = crate::app::copy_columns(statement, fields)?;
    for name in names {
        let copied = columns.iter().any(|column| match column {
            CopyColumn::Field(field) => field.name == *name,
            CopyColumn::RowId => name == "id" || name == "_id",
        });
        if copied {
            continue;
        }
        let exists = fields.iter().any(|field| field.name == *name)
            || crate::types::row_identity::declares_id(fields.iter().map(|f| f.name.as_str()))
                && (name == "id" || name == "_id");
        let (code, message) = if exists {
            (
                "42P10",
                format!("{option} column \"{name}\" not referenced by COPY"),
            )
        } else {
            ("42703", format!("column \"{name}\" does not exist"))
        };
        return Err(crate::app::DiagnosticError::new(code, message).into());
    }
    Ok(())
}

fn validate_header(
    columns: &[CopyColumn],
    mode: CopyHeaderMode,
    rows: &[Vec<Option<String>>],
) -> Result<(), CopyOperationError> {
    if mode != CopyHeaderMode::Match || rows.is_empty() {
        return Ok(());
    }
    let matches = rows[0].len() == columns.len()
        && rows[0].iter().zip(columns).all(|(header, column)| {
            let expected = match column {
                CopyColumn::Field(field) => field.name.as_str(),
                CopyColumn::RowId => "id",
            };
            header.as_deref() == Some(expected)
        });
    if !matches {
        return Err(crate::app::DiagnosticError::new(
            "22P04",
            "COPY CSV header does not match the target column list",
        )
        .into());
    }
    Ok(())
}
