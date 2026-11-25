//! READ COMMITTED may retarget a failed proposal; RR never retries it.

use super::{execute_insert_conflict_update, find_insert_conflict_row_id};
use super::{Cassie, CassieSession, InsertExecutionContext, QueryError, QueryExecutionControls};
use crate::app::CassieError;

pub(crate) fn resolve_transaction_conflict_intents(
    cassie: &Cassie,
    session: &CassieSession,
    controls: Option<&QueryExecutionControls>,
) -> Result<(), QueryError> {
    let fallback_controls = controls
        .is_none()
        .then(|| cassie.runtime.query_controls(std::time::Instant::now()));
    let controls = controls
        .or(fallback_controls.as_ref())
        .expect("fallback controls are present when caller controls are absent");
    for intent in session.transaction_conflict_intents() {
        session.remove_document_change(&intent.statement.table, &intent.provisional_id);
        let context = InsertExecutionContext {
            cassie,
            session: Some(session),
            statement: &intent.statement,
            params: &intent.params,
            user_functions: &intent.user_functions,
            schema: &intent.schema,
            controls,
            apply_defaults: false,
            conflict_images: None,
        };
        let Some(conflict_id) = find_insert_conflict_row_id(
            cassie,
            Some(session),
            &intent.statement,
            &intent.payload,
            &intent.schema,
        )?
        else {
            session
                .stage_document_write(
                    &intent.statement.table,
                    intent.provisional_id,
                    intent.payload,
                )
                .map_err(QueryError::from)?;
            continue;
        };
        let on_conflict = intent
            .statement
            .on_conflict
            .as_ref()
            .expect("transaction conflict intent has a conflict clause");
        if let crate::sql::ast::InsertConflictAction::DoUpdate {
            assignments,
            filter,
            ..
        } = &on_conflict.action
        {
            execute_insert_conflict_update(
                &context,
                &intent.payload,
                &conflict_id,
                assignments,
                filter.as_ref(),
            )?;
        }
    }
    session.clear_conflict_intents();
    Ok(())
}

/// Called only after a known failed native publication. The enclosing RR
/// COMMIT retires the entire transaction, so removing provisional own rows is
/// diagnostic lookup preparation, never a second publication attempt.
pub(crate) fn repeatable_conflict_failure(
    cassie: &Cassie,
    session: &CassieSession,
    original: CassieError,
    controls: Option<&QueryExecutionControls>,
) -> CassieError {
    let fallback = controls
        .is_none()
        .then(|| cassie.runtime.query_controls(std::time::Instant::now()));
    let controls = controls
        .or(fallback.as_ref())
        .expect("current commit controls");
    let diagnosed: Result<(), QueryError> = (|| {
        for intent in session.transaction_conflict_intents() {
            session.remove_document_change(&intent.statement.table, &intent.provisional_id);
            if let Some(id) = find_insert_conflict_row_id(
                cassie,
                Some(session),
                &intent.statement,
                &intent.payload,
                &intent.schema,
            )? {
                cassie.check_repeatable_conflict_row(
                    session,
                    &intent.statement.table,
                    &id,
                    controls,
                )?;
            }
        }
        Ok(())
    })();
    diagnosed.err().map_or(original, CassieError::from)
}
