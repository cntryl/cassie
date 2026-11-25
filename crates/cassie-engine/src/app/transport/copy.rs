//! Engine-owned admission and publication for a decoded COPY FROM operation.

use std::sync::Arc;
use std::time::Instant;

use crate::app::{
    Cassie, CassieError, CassieSession, CopyColumn, CopyCommandOwner, CopyIngestOptions,
    CopyIngestResult, FailureAuthority,
};
use crate::runtime::{QueryCancellationHandle, QueryExecutionControls, QueryMemoryReservation};
use crate::sql::ast::{CopyStatement, QueryStatement};

#[cfg(any(test, feature = "transport-test-hooks"))]
#[path = "copy/binding_accounting_test_hooks.rs"]
mod binding_accounting_test_hooks;
#[path = "copy/filter.rs"]
mod filter;
#[path = "copy/filter_text.rs"]
mod filter_text;
#[cfg(any(test, feature = "transport-test-hooks"))]
#[path = "copy/owner_check_test_hooks.rs"]
mod owner_check_test_hooks;
#[cfg(any(test, feature = "transport-test-hooks"))]
#[path = "copy/publication_memory_test_hooks.rs"]
mod publication_memory_test_hooks;

#[cfg(any(test, feature = "transport-test-hooks"))]
pub mod test_hooks {
    pub use super::binding_accounting_test_hooks::{
        cancel_after_validation, expire_after_validation, leave_before_charge, Observation,
        Remaining,
    };
    pub use super::owner_check_test_hooks::{install_after_owner_check, Guard};
    pub use super::publication_memory_test_hooks::{observe_before_ingest, Guard as MemoryGuard};
}

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
    database: String,
    owner: Option<CopyCommandOwner>,
    authority: FailureAuthority,
    controls: QueryExecutionControls,
    admission: QueryMemoryReservation,
}

impl CopyFromOperation {
    /// Binds, validates, and admits a COPY FROM target before accepting rows.
    ///
    /// # Errors
    /// Returns parse, authorization, transaction, schema, deadline, or memory errors.
    ///
    /// # Panics
    /// Panics if internal owner/context selection produces neither an owned nor a fallback binding context.
    pub fn bind(
        cassie: &Cassie,
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
        let database = owned_context.as_ref().map_or_else(
            || {
                session
                    .current_database()
                    .unwrap_or(cassie.default_database())
                    .to_owned()
            },
            |context| context.context().database.clone(),
        );
        let fallback_context = fallback_copy_context(cassie, session, owner.is_none());
        let context = owned_context
            .as_ref()
            .map(crate::app::CopyBindingContext::context)
            .or(fallback_context.as_ref())
            .expect("COPY binding context");
        let (scoped, statement) = bind_copy_target(
            cassie,
            session,
            parsed,
            owner.as_ref(),
            context,
            force_null,
            force_not_null,
        )?;
        let admission_bytes = copy_admission_bytes(owner.as_ref(), database.capacity())?;
        authority.reserve_shared_owner(&controls)?;
        #[cfg(any(test, feature = "transport-test-hooks"))]
        let mut observed = binding_accounting_test_hooks::before_admission(
            cassie,
            session,
            &mut controls,
            admission_bytes,
            owner.is_some(),
        );
        let interruption = controls.check_interruption();
        #[cfg(any(test, feature = "transport-test-hooks"))]
        if interruption.is_err() {
            if let Some(observed) = observed.take() {
                observed.finish(false);
            }
        }
        interruption?;
        let admission = controls.reserve_query_memory(admission_bytes);
        #[cfg(any(test, feature = "transport-test-hooks"))]
        if let Some(observed) = observed {
            observed.finish(true);
        }
        let admission = admission?;
        Ok(Self {
            cassie: Arc::new(scoped),
            statement,
            database,
            owner,
            authority,
            controls,
            admission,
        })
    }

    /// Request controls associated with the operation, borrowed for decoding.
    #[must_use]
    pub fn controls(&self) -> &QueryExecutionControls {
        debug_assert!(self.admission.belongs_to(&self.controls));
        &self.controls
    }

    /// Runs transport decoding and semantic execution under this operation's
    /// publication controls without exposing the storage adapter scope.
    pub fn with_publication_scope<T>(
        &self,
        operation: impl FnOnce(&QueryExecutionControls) -> T,
    ) -> T {
        let _scope = crate::midge::adapter::PublicationControlScope::enter(Some(&self.controls));
        let controls = crate::midge::adapter::PublicationControlScope::current_query_controls()
            .unwrap_or_else(|| self.controls.clone());
        operation(&controls)
    }

    #[cfg(any(test, feature = "transport-test-hooks"))]
    #[doc(hidden)]
    pub fn expire_for_test(&mut self) {
        self.controls.deadline = Some(Instant::now());
    }

    #[cfg(any(test, feature = "transport-test-hooks"))]
    #[doc(hidden)]
    #[must_use]
    pub fn cassie_for_test(&self) -> &Arc<Cassie> {
        &self.cassie
    }

    #[cfg(any(test, feature = "transport-test-hooks"))]
    #[doc(hidden)]
    #[must_use]
    pub fn has_owner_for_test(&self) -> bool {
        self.owner.is_some()
    }

    #[cfg(any(test, feature = "transport-test-hooks"))]
    #[doc(hidden)]
    #[must_use]
    pub fn authority_for_test(&self) -> FailureAuthority {
        self.authority.clone()
    }

    #[cfg(any(test, feature = "transport-test-hooks"))]
    #[doc(hidden)]
    pub fn controls_for_test_mut(&mut self) -> &mut QueryExecutionControls {
        &mut self.controls
    }

    #[cfg(any(test, feature = "transport-test-hooks"))]
    #[doc(hidden)]
    #[must_use]
    pub fn into_admission_reservation_for_test(self) -> QueryMemoryReservation {
        self.admission
    }

    #[cfg(any(test, feature = "transport-test-hooks"))]
    #[doc(hidden)]
    #[must_use]
    pub fn admission_reservation_bytes_for_test(&self) -> usize {
        self.admission.bytes_for_test()
    }

    #[cfg(any(test, feature = "transport-test-hooks"))]
    #[doc(hidden)]
    #[must_use]
    pub fn admission_reservation_belongs_to_for_test(
        &self,
        controls: &QueryExecutionControls,
    ) -> bool {
        self.admission.belongs_to(controls)
    }

    #[cfg(any(test, feature = "transport-test-hooks"))]
    #[doc(hidden)]
    #[must_use]
    pub fn owner_reservation_matches_for_test(&self) -> Option<bool> {
        self.owner
            .as_ref()
            .map(|owner| owner.reservation_matches_for_test(&self.controls))
    }

    #[cfg(any(test, feature = "transport-test-hooks"))]
    #[doc(hidden)]
    #[must_use]
    pub fn shared_authority_reservation_matches_for_test(&self) -> bool {
        self.authority
            .shared_owner_reservation_matches_for_test(&self.controls)
    }

    /// Bound target statement used by the server's wire decoder.
    #[must_use]
    pub fn statement(&self) -> &CopyStatement {
        &self.statement
    }

    /// Typed target columns used by transport format decoders.
    ///
    /// # Errors
    /// Returns an error if the admitted target schema is no longer present or its column list is invalid.
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
    ///
    /// # Errors
    /// Returns an error if the admitted target schema is no longer present or the column list is invalid.
    pub fn force_null_indices(
        &self,
        selection: &CopyForceSelection,
    ) -> Result<Vec<usize>, CassieError> {
        Ok(force_indices(selection, &self.target_columns()?))
    }

    /// Column positions forced to non-null by CSV decoding.
    ///
    /// # Errors
    /// Returns an error if the admitted target schema is no longer present or the column list is invalid.
    pub fn force_not_null_indices(
        &self,
        selection: &CopyForceSelection,
    ) -> Result<Vec<usize>, CassieError> {
        Ok(force_indices(selection, &self.target_columns()?))
    }

    /// Supplies binary wire decoder type metadata from the same scoped catalog
    /// and database used to bind this operation. The descriptor allocation
    /// remains admitted until the callback returns.
    ///
    /// # Errors
    /// Returns an error if memory reservation, interruption checks, or target-column lookup fails.
    pub fn with_binary_columns<T>(
        &self,
        controls: &QueryExecutionControls,
        decode: impl FnOnce(&[CopyBinaryColumn]) -> T,
    ) -> Result<T, CassieError> {
        let schema = self
            .cassie
            .catalog
            .get_schema(&self.statement.table)
            .ok_or_else(|| CassieError::Parse("COPY target schema disappeared".into()))?;
        let base = controls.reserve_query_memory(0)?;
        let fields = schema.fields.as_slice();
        let count = if self.statement.columns.is_empty() {
            fields.len()
        } else {
            self.statement.columns.len()
        };
        let (mut metadata, _metadata_memory) =
            crate::runtime::owned_memory::try_reserve_vec(count, &base)?;
        let declares_id =
            crate::types::row_identity::declares_id(fields.iter().map(|field| field.name.as_str()));
        if self.statement.columns.is_empty() {
            for field in fields {
                controls.check_interruption()?;
                metadata.push(self.binary_column(&field.data_type)?);
            }
        } else {
            for column in &self.statement.columns {
                controls.check_interruption()?;
                if crate::types::row_identity::is_identity_reference(column, declares_id) {
                    metadata.push(self.binary_column(&crate::types::DataType::Text)?);
                    continue;
                }
                let field = fields
                    .iter()
                    .find(|field| field.name == *column)
                    .or_else(|| {
                        fields.iter().find(|field| {
                            crate::sql::ColumnIdentifierPath::matches_stored_field(
                                column,
                                &field.name,
                            )
                        })
                    })
                    .ok_or_else(|| {
                        CassieError::Planner(format!(
                            "COPY target column '{column}' does not exist in '{}'",
                            self.statement.table
                        ))
                    })?;
                metadata.push(self.binary_column(&field.data_type)?);
            }
        }
        let result = decode(&metadata);
        Ok(result)
    }

    fn binary_column(
        &self,
        data_type: &crate::types::DataType,
    ) -> Result<CopyBinaryColumn, CassieError> {
        let (oid, is_vector, expected_vector_dimensions) = match data_type {
            crate::types::DataType::Enum(_) => {
                (crate::types::DataType::Text.type_oid(), false, None)
            }
            crate::types::DataType::Vector(dimensions) => {
                let oid = crate::catalog::object_identity::type_oid_for_database(
                    &self.cassie.catalog,
                    &self.database,
                    data_type,
                )
                .ok_or_else(|| {
                    CassieError::Unsupported(
                        "vector type is not installed in the active database".into(),
                    )
                })?;
                (oid, true, *dimensions)
            }
            _ => (data_type.type_oid(), false, None),
        };
        Ok(CopyBinaryColumn {
            oid,
            is_vector,
            expected_vector_dimensions,
        })
    }

    /// Validates and publishes decoded rows under the retained owner authority.
    ///
    /// # Errors
    /// Returns row format, stale owner, WHERE, or publication errors.
    pub fn execute_decoded_rows(
        &self,
        session: &CassieSession,
        rows: Vec<Vec<Option<String>>>,
        options: &CopyExecutionOptions<'_>,
        cancellation: Option<&QueryCancellationHandle>,
    ) -> Result<CopyIngestResult, CopyOperationError> {
        self.execute_decoded_rows_with_controls(
            session,
            rows,
            options,
            cancellation,
            &self.controls,
        )
    }

    /// Executes rows using the controls active in the originating request scope.
    ///
    /// # Errors
    /// Returns an error for invalid headers, stale transaction ownership, failed row filtering, or publication failures.
    pub fn execute_decoded_rows_with_controls(
        &self,
        session: &CassieSession,
        mut rows: Vec<Vec<Option<String>>>,
        options: &CopyExecutionOptions<'_>,
        cancellation: Option<&QueryCancellationHandle>,
        controls: &QueryExecutionControls,
    ) -> Result<CopyIngestResult, CopyOperationError> {
        let columns = self.target_columns()?;
        validate_header(&columns, options.header, &rows)?;
        if options.header != CopyHeaderMode::None && !rows.is_empty() {
            rows.remove(0);
        }
        let operation_session = session.with_operation_failure_authority(Some(&self.authority));
        if let Some(owner) = &self.owner {
            owner.ensure_current_transaction(&operation_session, controls)?;
            #[cfg(any(test, feature = "transport-test-hooks"))]
            owner_check_test_hooks::after_owner_check(&self.cassie, session);
        }
        let filter_rows_memory;
        if let Some(predicate) = options.where_clause {
            let (accepted, memory) = filter::filter_rows_for_operation(
                &self.cassie,
                &operation_session,
                &self.statement,
                rows,
                predicate,
                (controls, Some(&self.authority)),
            )?;
            rows = accepted;
            filter_rows_memory = Some(memory);
        } else {
            filter_rows_memory = None;
        }
        #[cfg(any(test, feature = "transport-test-hooks"))]
        publication_memory_test_hooks::before_ingest(controls);
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
        drop(filter_rows_memory);
        Ok(result)
    }
}

fn fallback_copy_context(
    cassie: &Cassie,
    session: &CassieSession,
    needed: bool,
) -> Option<crate::sql::binder::BindingContext> {
    if !needed {
        return None;
    }
    let database = session
        .current_database()
        .unwrap_or(cassie.default_database())
        .to_owned();
    let search_path = cassie.binding_search_path(session);
    Some(if cassie.database_catalog_enforced() {
        crate::sql::binder::BindingContext::scoped(database, search_path)
    } else {
        crate::sql::binder::BindingContext::unscoped(database, search_path)
    })
}

fn bind_copy_target(
    cassie: &Cassie,
    session: &CassieSession,
    parsed: crate::sql::ast::ParsedStatement,
    owner: Option<&CopyCommandOwner>,
    context: &crate::sql::binder::BindingContext,
    force_null: &CopyForceSelection,
    force_not_null: &CopyForceSelection,
) -> Result<(Cassie, CopyStatement), CassieError> {
    let scoped = owner.map_or_else(
        || cassie.clone(),
        |owner| cassie.with_copy_command_owner(session, owner),
    );
    session.mark_transaction_query();
    let bound = crate::sql::binder::bind_with_context(parsed, &scoped.catalog, context)?;
    let QueryStatement::Copy(statement) = bound.statement.statement else {
        return Err(CassieError::Parse(
            "COPY FROM STDIN did not bind as a table copy".into(),
        ));
    };
    if owner.is_some_and(|owner| !owner.matches_target(&statement.table)) {
        return Err(CassieError::Execution(
            "COPY target changed during binding".into(),
        ));
    }
    let schema = scoped
        .catalog
        .get_schema(&statement.table)
        .ok_or_else(|| CassieError::Parse("COPY target schema disappeared".into()))?;
    validate_force_columns(&statement, &schema.fields, force_null, "FORCE_NULL")?;
    validate_force_columns(&statement, &schema.fields, force_not_null, "FORCE_NOT_NULL")?;
    Ok((scoped, statement))
}

fn copy_admission_bytes(
    owner: Option<&CopyCommandOwner>,
    database_capacity: usize,
) -> Result<usize, CassieError> {
    let owner_bytes = owner.map_or(0, |_| std::mem::size_of::<CopyCommandOwner>());
    std::mem::size_of::<CopyFromOperation>()
        .checked_sub(owner_bytes)
        .ok_or_else(|| CassieError::ResourceLimit("COPY binding size underflow".into()))?
        .checked_add(database_capacity)
        .ok_or_else(|| CassieError::ResourceLimit("COPY binding size overflow".into()))
}

/// Schema-derived type descriptor for the PostgreSQL binary COPY decoder.
#[derive(Clone, Copy, Debug)]
pub struct CopyBinaryColumn {
    pub oid: i64,
    pub is_vector: bool,
    pub expected_vector_dimensions: Option<usize>,
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

#[cfg(test)]
mod tests {
    use super::{CopyExecutionOptions, CopyForceSelection, CopyFromOperation, CopyHeaderMode};
    use crate::app::Cassie;
    use crate::config::CassieRuntimeConfig;

    #[test]
    fn should_retain_where_filter_memory_through_copy_publication() {
        // Arrange
        let cassie = Cassie::new_in_memory_with_config(CassieRuntimeConfig::default())
            .expect("memory fixture");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE copy_filter_lease (id INT)",
                Vec::new(),
            )
            .expect("target table");
        cassie
            .execute_sql(&session, "BEGIN", Vec::new())
            .expect("transaction");
        let operation = CopyFromOperation::bind(
            &cassie,
            &session,
            "COPY copy_filter_lease (id) FROM STDIN",
            &CopyForceSelection::None,
            &CopyForceSelection::None,
            None,
        )
        .expect("admitted COPY");
        let controls = operation.controls().clone();
        let retained_before_decode = controls.current_query_memory_bytes();
        let (_guard, observed) = super::test_hooks::observe_before_ingest();

        // Act
        let result = operation.with_publication_scope(|active_controls| {
            operation.execute_decoded_rows_with_controls(
                &session,
                vec![vec![Some("41".to_owned())]],
                &CopyExecutionOptions {
                    header: CopyHeaderMode::None,
                    where_clause: Some("id > 0"),
                    default_marker: None,
                    on_error_ignore: false,
                    reject_limit: None,
                },
                None,
                active_controls,
            )
        });
        let retained_during_publication = observed.recv().expect("pre-ingest memory observation");

        // Assert
        assert_eq!(result.expect("COPY publication").count, 1);
        assert!(retained_during_publication > retained_before_decode);
        cassie
            .execute_sql(&session, "ROLLBACK", Vec::new())
            .expect("rollback");
    }
}
