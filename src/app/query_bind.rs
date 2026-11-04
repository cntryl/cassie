//! Validate executable plans at Bind without reading rows or applying writes.
use std::time::Instant;

use super::{Cassie, CassieError, CassieSession};
use crate::runtime::{ExecutionMode, QueryCancellationHandle, QueryExecutionControls};
use crate::sql::{ParsedStatement, QueryStatement};

impl Cassie {
    pub(crate) fn validate_pgwire_bind(
        &self,
        session: &CassieSession,
        parsed: ParsedStatement,
        sql_fingerprint: u64,
        parameter_types: &[i32],
        parameters: &[crate::types::Value],
        authority: (&QueryCancellationHandle, &QueryExecutionControls),
    ) -> Result<Option<std::sync::Arc<super::BoundReadView>>, CassieError> {
        let (cancellation, retention) = authority;
        let started = Instant::now();
        let mut controls = QueryExecutionControls::with_cancellation(
            &self.runtime.limits(),
            started,
            cancellation.clone(),
        );
        session.apply_statement_timeout(&mut controls, started);
        let result = self
            .validate_pgwire_bind_with_controls(
                session,
                parsed,
                sql_fingerprint,
                parameter_types,
                parameters,
                &controls,
            )
            .and_then(|needs_data| {
                needs_data
                    .map(|needs_data| {
                        session.capture_bound_read_view(
                            &self.midge,
                            session.current_database().unwrap_or(&self.default_database),
                            needs_data,
                            &controls,
                            retention,
                        )
                    })
                    .transpose()
                    .map(Option::flatten)
            });
        self.runtime.record_query_memory(&controls);
        result
    }

    fn validate_pgwire_bind_with_controls(
        &self,
        session: &CassieSession,
        parsed: ParsedStatement,
        sql_fingerprint: u64,
        parameter_types: &[i32],
        parameters: &[crate::types::Value],
        controls: &QueryExecutionControls,
    ) -> Result<Option<bool>, CassieError> {
        self.check_storage_statement_admission(&parsed.statement)?;
        if self.check_storage_recovery_required().is_err() {
            return Ok(None);
        }
        self.prepare_transaction_analysis_view(session, &parsed, !parameters.is_empty(), controls)?;
        self.ensure_session_database_access(session)?;
        self.ensure_statement_can_execute(session, &parsed, controls)?;
        // Utility statements, notably EXPLAIN, plan their contained query when
        // executed. Bind must neither perform that work nor execute commands.
        if !matches!(
            parsed.statement,
            QueryStatement::Select(_)
                | QueryStatement::Insert(_)
                | QueryStatement::Update(_)
                | QueryStatement::Delete(_)
        ) {
            return Ok(
                (Self::analysis_requires_snapshot(&parsed) || !parameters.is_empty())
                    .then_some(true),
            );
        }
        let _publication = self.catalog_publication.read();
        let descriptor_owner =
            session.command_descriptor_snapshot(&self.publication_catalog, controls)?;
        let catalog_overlay = session.transaction_catalog_overlay();
        let mut scoped_cassie = self.clone();
        scoped_cassie.catalog_owner = descriptor_owner.clone();
        scoped_cassie.metadata_write_catalog = session
            .transaction_catalog_overlay()
            .unwrap_or_else(|| self.publication_catalog.clone());
        scoped_cassie.catalog = self.publication_catalog.clone();
        if let Some(owner) = &descriptor_owner {
            scoped_cassie.catalog = owner.catalog().clone();
        } else if let Some(catalog) = catalog_overlay.as_ref() {
            scoped_cassie.catalog = catalog.clone();
        }
        let physical =
            if matches!(parsed.statement, QueryStatement::Select(_)) && catalog_overlay.is_none() {
                let key = self.plan_cache_key_from_fingerprint(
                    sql_fingerprint,
                    parameter_types,
                    ExecutionMode::ExtendedQuery,
                    session.database.clone(),
                    &session.search_path(),
                );
                scoped_cassie
                    .resolve_physical_plan_with_parameter_oids(
                        parsed,
                        key,
                        Some(session),
                        Some(controls),
                        parameter_types,
                    )?
                    .0
            } else {
                scoped_cassie.compile_physical_plan_with_parameter_oids(
                    parsed,
                    Some(session),
                    Some(controls),
                    parameter_types,
                )?
            };
        crate::executor::check_bound_plan_constants(&physical.logical, controls, parameters)?;
        controls.check_interruption()?;
        Ok(Some(
            physical.logical.command.is_some()
                || crate::executor::plan_needs_statement_data(&physical.logical),
        ))
    }
}

#[cfg(test)]
#[path = "query_bind_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "query_bind/read_view_tests.rs"]
mod read_view_tests;
