//! Acquire a retained view without extending idle or READ COMMITTED ownership.

use std::time::Duration;

use super::{Cassie, CassieError, CassieSession, QueryExecutionControls, QueryStatement};
use crate::sql::ast::SessionObjectStatement;
use crate::sql::ParsedStatement;

impl Cassie {
    pub(crate) fn prepare_transaction_read_view(
        &self,
        session: &CassieSession,
        parsed: &ParsedStatement,
        controls: &QueryExecutionControls,
    ) -> Result<(), CassieError> {
        if matches!(
            parsed.statement,
            QueryStatement::Transaction(_)
                | QueryStatement::Set(_)
                | QueryStatement::Show(_)
                | QueryStatement::SessionObject(SessionObjectStatement::Fetch(_))
        ) {
            return Ok(());
        }
        if session.repeatable_read_capture_generation().is_some() {
            self.ensure_statement_can_execute(session, parsed, controls)?;
        }
        self.capture_transaction_read_view(session, parsed, controls)
    }

    pub(crate) fn prepare_transaction_analysis_view(
        &self,
        session: &CassieSession,
        parsed: &ParsedStatement,
        has_parameters: bool,
        controls: &QueryExecutionControls,
    ) -> Result<(), CassieError> {
        self.check_storage_statement_admission(&parsed.statement)?;
        // PostgreSQL18.6 analyze_requires_snapshot and exec_bind_message:
        // utility wrappers acquire during analysis only in these cases; Bind
        // also needs a snapshot for parameter input functions.
        let requires_analysis = match &parsed.statement {
            QueryStatement::Select(_)
            | QueryStatement::Insert(_)
            | QueryStatement::Update(_)
            | QueryStatement::Delete(_)
            | QueryStatement::Merge(_)
            | QueryStatement::Explain(_)
            | QueryStatement::CallProcedure(_)
            | QueryStatement::SessionObject(SessionObjectStatement::DeclareCursor(_)) => true,
            QueryStatement::CreateTable(statement) => statement.as_query.is_some(),
            _ => false,
        };
        if !requires_analysis && !has_parameters {
            return Ok(());
        }
        self.capture_transaction_read_view(session, parsed, controls)
    }

    pub(crate) fn check_repeatable_conflict_row(
        &self,
        session: &CassieSession,
        collection: &str,
        id: &str,
        controls: &QueryExecutionControls,
    ) -> Result<(), CassieError> {
        let Some(history) = session.repeatable_conflict_history(collection, id) else {
            return Ok(());
        };
        let relation = self
            .catalog
            .collection_storage_id(collection)
            .filter(|identity| *identity != 0)
            .ok_or_else(|| {
                CassieError::Unsupported(
                    "REPEATABLE READ conflict validation requires a persisted relation identity"
                        .to_owned(),
                )
            })?;
        let database = session.current_database().unwrap_or(&self.default_database);
        history.check_row(&self.midge, database, (collection, relation, id), controls)
    }

    fn capture_transaction_read_view(
        &self,
        session: &CassieSession,
        parsed: &ParsedStatement,
        controls: &QueryExecutionControls,
    ) -> Result<(), CassieError> {
        if session.repeatable_read_capture_generation().is_none() {
            return Ok(());
        }
        self.ensure_session_database_access(session)?;
        Self::ensure_transaction_recovery(session, parsed)?;
        session.authorize_statement(&parsed.statement)?;
        // Existing executable statements hold the publication read guard. A
        // brief exclusive capture keeps data and SQL metadata at one boundary.
        let _publication = loop {
            controls.check_interruption()?;
            #[cfg(test)]
            tests::before_publication_lock();
            if let Some(guard) = self
                .catalog_publication
                .try_write_for(Duration::from_millis(2))
            {
                break guard;
            }
        };
        let Some(generation) = session.repeatable_read_capture_generation() else {
            return Ok(());
        };
        let database = session.current_database().unwrap_or(&self.default_database);
        let (data, catalog, fence, history) =
            self.midge
                .with_database_publication(database, controls, || {
                    let history = session
                        .is_repeatable_read_write()
                        .then(|| {
                            self.midge
                                .capture_repeatable_row_history(database, controls)
                        })
                        .transpose()?;
                    let fence = session
                        .is_repeatable_read_write()
                        .then(|| {
                            crate::midge::adapter::RetainedRowFence::capture(
                                &self.midge,
                                database,
                                controls,
                            )
                        })
                        .transpose()?;
                    let data = session.capture_statement_read(&self.midge, database, controls)?;
                    let catalog = self.catalog.snapshot_with_controls(controls)?;
                    controls.check_interruption()?;
                    Ok((data, catalog, fence, history))
                })?;
        session
            .install_repeatable_read_snapshot(generation, data, catalog, fence, history, controls)
    }
}

#[cfg(test)]
mod tests;
