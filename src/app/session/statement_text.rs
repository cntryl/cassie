//! Immutable statement text belongs to one execution, never shared session state.
use super::CassieSession;
use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::sql::ast::Expr;
use std::sync::Arc;

#[derive(Debug)]
pub(super) struct StatementText {
    sql: String,
    controls: QueryExecutionControls,
    _memory: QueryMemoryReservation,
}

impl CassieSession {
    pub(crate) fn with_statement_text(
        &self,
        sql: &str,
        controls: &QueryExecutionControls,
    ) -> Result<Self, CassieError> {
        controls.check_interruption()?;
        crate::sql::parser::preflight_sql(sql)?;
        let memory = controls.reserve_query_memory(
            sql.len()
                .checked_add(self.user.len())
                .and_then(|bytes| bytes.checked_add(self.database.as_ref().map_or(0, String::len)))
                .and_then(|bytes| {
                    bytes.checked_add(
                        std::mem::size_of::<StatementText>() + 2 * std::mem::size_of::<usize>(),
                    )
                })
                .ok_or_else(|| CassieError::ResourceLimit("statement text size overflow".into()))?,
        )?;
        self.install_temporal_session(controls);
        let mut session = self.clone();
        session.statement_text = Some(Arc::new(StatementText {
            sql: sql.to_owned(),
            controls: controls.clone(),
            _memory: memory,
        }));
        Ok(session)
    }

    pub(crate) fn statement_execution_controls(&self) -> Option<&QueryExecutionControls> {
        self.statement_text.as_ref().map(|text| &text.controls)
    }

    pub(crate) fn check_statement_interruption(&self) -> Result<(), CassieError> {
        self.statement_text
            .as_ref()
            .map_or(Ok(()), |text| text.controls.check_interruption())
    }

    pub(crate) fn statement_text(&self) -> Option<&str> {
        self.statement_text.as_ref().map(|text| text.sql.as_str())
    }

    // Admit evaluator copies before constructing Strings. Count all function
    // nodes so callers through stored SQL functions also have a scratch owner.
    pub(crate) fn statement_text_scratch(
        &self,
        expression: &Expr,
    ) -> Result<Option<QueryMemoryReservation>, CassieError> {
        let Some(text) = &self.statement_text else {
            return Ok(None);
        };
        text.controls.check_interruption()?;
        let mut count = 0_usize;
        expression.any_descendant_or_self(&mut |expr| {
            if matches!(expr, Expr::Function(_)) {
                count = count.saturating_add(1);
            }
            false
        });
        let bytes = text
            .sql
            .len()
            .checked_mul(count)
            .and_then(|bytes| bytes.checked_mul(4))
            .ok_or_else(|| CassieError::ResourceLimit("statement text scratch overflow".into()))?;
        text.controls.reserve_query_memory(bytes).map(Some)
    }
}
