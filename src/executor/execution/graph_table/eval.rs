//! Evaluation of inline predicates, the pattern `WHERE` and `COLUMNS` with the ordinary SQL
//! expression evaluator.

use std::sync::Arc;

use super::BatchRow;
use super::{QueryError, SourceExecutionEnv};
use crate::executor::filter::{evaluate_source_argument, EvaluationMemory};
use crate::executor::retained_memory::{add, lookup_bytes, mul, value_clone_bytes};
use crate::runtime::accounted::{Accounted, AccountedVec};
use crate::sql::ast::Expr;
use crate::types::Value;
use std::mem::size_of;

pub(super) type Entries = Vec<(String, Value)>;

pub(super) struct Evaluator<'a> {
    env: &'a SourceExecutionEnv<'a>,
    outer: Option<Arc<BatchRow>>,
    _outer_memory: Option<crate::runtime::QueryMemoryReservation>,
}

/// Bytes a pattern-expression row retains while it is evaluated.
fn entries_bytes(entries: &Entries) -> Result<usize, crate::app::CassieError> {
    let names = entries
        .iter()
        .try_fold(0, |bytes, (name, _)| add(bytes, name.len()))?;
    entries.iter().try_fold(
        add(
            size_of::<BatchRow>(),
            add(
                mul(entries.len(), size_of::<(String, Value)>())?,
                lookup_bytes(entries.len(), names)?,
            )?,
        )?,
        |bytes, (name, value)| add(bytes, add(name.len(), value_clone_bytes(value)?)?),
    )
}

pub(super) fn push_entry(
    entries: &mut AccountedVec<(String, Value)>,
    name: &str,
    value: Option<&Value>,
) -> Result<(), QueryError> {
    let heap = value.map_or(Ok(0), value_clone_bytes)?;
    entries.try_push_with(add(name.len(), heap)?, || {
        (name.to_owned(), value.cloned().unwrap_or(Value::Null))
    })?;
    Ok(())
}

pub(super) fn combine_entries(
    env: &SourceExecutionEnv<'_>,
    left: &Entries,
    right: &Entries,
) -> Result<Accounted<Entries>, QueryError> {
    let mut entries = AccountedVec::try_new(env.controls)?;
    for (name, value) in left.iter().chain(right) {
        env.controls.check_interruption()?;
        push_entry(&mut entries, name, Some(value))?;
    }
    let (entries, memory) = entries.into_parts();
    Ok(Accounted::from_admitted(entries, memory))
}

impl<'a> Evaluator<'a> {
    pub(super) fn new(
        env: &'a SourceExecutionEnv<'a>,
        outer: Option<&BatchRow>,
    ) -> Result<Self, QueryError> {
        let memory = outer
            .map(|row| {
                env.controls.reserve_query_memory(add(
                    size_of::<BatchRow>() + 2 * size_of::<usize>(),
                    row.owned_body_bytes()?,
                )?)
            })
            .transpose()?;
        Ok(Self {
            env,
            outer: outer.map(|row| Arc::new(row.clone())),
            _outer_memory: memory,
        })
    }

    /// The value of `expr` over the variable properties in `entries`. Columns that name no
    /// entry resolve against the outer row of a `LATERAL` source.
    pub(super) fn value(
        &self,
        expr: &Expr,
        entries: &Entries,
    ) -> Result<Accounted<Value>, QueryError> {
        let env = self.env;
        env.controls.check_interruption()?;
        let memory = EvaluationMemory::new(env.controls)?;
        memory.grow(entries_bytes(entries)?)?;
        let mut row = BatchRow::new(entries.clone());
        if let Some(outer) = &self.outer {
            row = row.with_outer_scope(Arc::clone(outer));
        }
        let value = evaluate_source_argument(
            &row,
            expr,
            env.params,
            env.user_functions,
            env.session,
            env.controls,
            &memory,
        )?;
        Ok(Accounted::from_admitted(value, memory.into_reservation()))
    }

    /// Whether `expr` is TRUE; NULL and FALSE both reject.
    pub(super) fn holds(&self, expr: &Expr, entries: &Entries) -> Result<bool, QueryError> {
        Ok(matches!(
            self.value(expr, entries)?.get(),
            Value::Bool(true)
        ))
    }
}

/// `columns` of one element's row as `variable.property` entries.
pub(super) fn element_entries(
    env: &SourceExecutionEnv<'_>,
    row: &BatchRow,
    columns: &[(String, String)],
) -> Result<Accounted<Entries>, QueryError> {
    let mut entries = AccountedVec::try_new(env.controls)?;
    for (entry, column) in columns {
        env.controls.check_interruption()?;
        push_entry(&mut entries, entry, row.get(column))?;
    }
    let (entries, memory) = entries.into_parts();
    Ok(Accounted::from_admitted(entries, memory))
}
