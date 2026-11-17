//! The pattern `WHERE` on selected paths and the `COLUMNS` projection.

use std::mem::size_of;

use super::bindings::{scalar_entries, Candidate};
use super::eval::Evaluator;
use super::path_value::PathBuilder;
use super::plan::Plan;
use super::store::Store;
use super::BatchRow;
use super::QueryError;
use super::SourceExecutionEnv;
use crate::executor::retained_memory::{add, value_clone_bytes};
use crate::runtime::QueryMemoryReservation;
use crate::sql::ast::Expr;

/// One output row per selected path whose pattern `WHERE` is TRUE.
///
/// # Errors
///
/// Expression errors, cancellation and the query memory budget.
pub(super) fn rows(
    env: &SourceExecutionEnv<'_>,
    plan: &Plan<'_>,
    store: &Store,
    eval: &Evaluator<'_>,
    selected: &[Candidate],
    memory: &mut QueryMemoryReservation,
) -> Result<Vec<BatchRow>, QueryError> {
    let table = plan.table;
    // The binder renames references to the output columns onto these positional names.
    let names = (0..table.columns.len())
        .map(crate::sql::table_sources::physical_name)
        .collect::<Vec<_>>();
    let is_path = |expr: &Expr| matches!((expr, &table.path_variable), (Expr::Column(name), Some(path)) if name == path);
    let paths = table
        .columns
        .iter()
        .any(|column| is_path(&column.expr))
        .then(|| PathBuilder::new(env, &plan.graph))
        .transpose()?;
    let mut out = Vec::new();
    for path in selected {
        env.controls.check_interruption()?;
        let entries = scalar_entries(plan, store, path);
        if let Some(filter) = &table.filter {
            if !eval.holds(filter, entries.clone())? {
                continue;
            }
        }
        #[cfg(test)]
        crate::executor::execution::graph_memory_test_observer::record(
            "before_graph_output",
            memory.bytes(),
        );
        #[cfg(test)]
        crate::executor::execution::graph_memory_test_observer::record(
            "copy_graph_output",
            memory.bytes(),
        );
        let mut values = Vec::with_capacity(names.len());
        let mut bytes = size_of::<BatchRow>() + 64;
        for (name, column) in names.iter().zip(&table.columns) {
            let value = match &paths {
                Some(builder) if is_path(&column.expr) => builder.value(env, store, path)?,
                _ => eval.value(&column.expr, entries.clone())?,
            };
            bytes = add(
                bytes,
                add(name.capacity() + 64, value_clone_bytes(&value)?)?,
            )?;
            values.push((name.clone(), value));
        }
        memory.try_grow(bytes)?;
        out.push(BatchRow::new(values));
    }
    Ok(out)
}

#[cfg(test)]
mod memory_tests {
    use crate::executor::execution::graph_memory_test_observer as observer;

    #[test]
    fn should_admit_graph_output_before_allocating_its_row_directory() {
        // Arrange
        let (cassie, session) = observer::fixture();
        let _observation = observer::start();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT * FROM GRAPH_TABLE(g MATCH(a IS node WHERE a.id=1)-[IS edge]->(b IS node) \
             COLUMNS(b.id)) AS t",
                vec![],
            )
            .expect("one graph path");

        // Assert
        assert_eq!(result.row_count(), 1);
        observer::assert_admitted(
            "before_graph_output",
            "copy_graph_output",
            std::mem::size_of::<super::BatchRow>(),
        );
    }
}
