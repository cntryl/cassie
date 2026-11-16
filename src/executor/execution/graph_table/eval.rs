//! Evaluation of inline predicates, the pattern `WHERE` and `COLUMNS` with the ordinary SQL
//! expression evaluator.

use std::sync::Arc;

use super::{QueryError, SourceExecutionEnv};
use crate::executor::filter::{evaluate_source_argument, EvaluationMemory};
use crate::executor::retained_memory::{add, value_clone_bytes};
use super::BatchRow;
use crate::sql::ast::Expr;
use crate::types::Value;

pub(super) type Entries = Vec<(String, Value)>;

pub(super) struct Evaluator<'a> {
    env: &'a SourceExecutionEnv<'a>,
    outer: Option<Arc<BatchRow>>,
}

/// Bytes a pattern-expression row retains while it is evaluated.
fn entries_bytes(entries: &Entries) -> Result<usize, crate::app::CassieError> {
    entries.iter().try_fold(256, |bytes, (name, value)| {
        add(add(bytes, name.capacity() + 64)?, value_clone_bytes(value)?)
    })
}

impl<'a> Evaluator<'a> {
    pub(super) fn new(env: &'a SourceExecutionEnv<'a>, outer: Option<&BatchRow>) -> Self {
        Self {
            env,
            outer: outer.map(|row| Arc::new(row.clone())),
        }
    }

    /// The value of `expr` over the variable properties in `entries`. Columns that name no
    /// entry resolve against the outer row of a `LATERAL` source.
    pub(super) fn value(&self, expr: &Expr, entries: Entries) -> Result<Value, QueryError> {
        let env = self.env;
        env.controls.check_interruption()?;
        let _scratch = env.controls.reserve_query_memory(entries_bytes(&entries)?)?;
        let mut row = BatchRow::new(entries);
        if let Some(outer) = &self.outer {
            row = row.with_outer_scope(Arc::clone(outer));
        }
        let memory = EvaluationMemory::new(env.controls)?;
        evaluate_source_argument(
            &row,
            expr,
            env.params,
            env.user_functions,
            env.session,
            env.controls,
            &memory,
        )
    }

    /// Whether `expr` is TRUE; NULL and FALSE both reject.
    pub(super) fn holds(&self, expr: &Expr, entries: Entries) -> Result<bool, QueryError> {
        Ok(matches!(self.value(expr, entries)?, Value::Bool(true)))
    }
}

/// `columns` of one element's row as `variable.property` entries.
pub(super) fn element_entries(row: &BatchRow, columns: &[(String, String)]) -> Entries {
    columns
        .iter()
        .map(|(entry, column)| (entry.clone(), row.get(column).cloned().unwrap_or(Value::Null)))
        .collect()
}
