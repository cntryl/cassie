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
use crate::executor::retained_memory::{
    add, data_type_clone_bytes, grown_capacity, lookup_bytes, mul,
};
use crate::runtime::owned_memory::grow_vec;
use crate::runtime::QueryMemoryReservation;
use crate::sql::ast::Expr;

/// Final source metadata and the replacement row/chunk directories coexist with
/// the projected row directory. Names use the ordinary `column{ordinal}` format.
pub(super) fn metadata_bytes(
    types: &[crate::types::DataType],
    row_count: usize,
) -> Result<usize, QueryError> {
    let shared = size_of::<Vec<crate::types::DataType>>()
        + size_of::<QueryMemoryReservation>()
        + 4 * size_of::<usize>();
    let mut bytes = add(
        shared,
        mul(types.len(), size_of::<crate::types::DataType>())?,
    )?;
    for data_type in types {
        bytes = add(bytes, data_type_clone_bytes(data_type)?)?;
    }
    bytes = add(bytes, mul(row_count, 2 * size_of::<BatchRow>())?)?;
    bytes = add(
        bytes,
        mul(
            row_count.div_ceil(super::batch::DEFAULT_BATCH_SIZE),
            size_of::<Vec<BatchRow>>(),
        )?,
    )?;
    let text_count = types
        .iter()
        .filter(|data_type| **data_type == crate::types::DataType::Text)
        .count();
    bytes = add(
        bytes,
        mul(grown_capacity(text_count, 4)?, size_of::<String>())?,
    )?;
    for (index, data_type) in types.iter().enumerate() {
        if *data_type == crate::types::DataType::Text {
            let digits = usize::try_from(add(index, 1)?.ilog10() + 1).unwrap_or(usize::MAX);
            // Formatting may retain geometric spare capacity in addition to the digits.
            bytes = add(bytes, mul(add("column".len(), digits)?, 2)?)?;
        }
    }
    Ok(bytes)
}

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
    let _name_memory = env
        .controls
        .reserve_query_memory(mul(table.columns.len(), size_of::<String>() + 32)?)?;
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
        let entries = scalar_entries(env, plan, store, path)?;
        if let Some(filter) = &table.filter {
            if !eval.holds(filter, entries.get())? {
                continue;
            }
        }
        #[cfg(test)]
        crate::executor::execution::graph_memory_test_observer::record(
            "before_graph_output",
            memory.bytes(),
        );
        let name_bytes = names
            .iter()
            .try_fold(0, |bytes, name| add(bytes, name.len()))?;
        memory.try_grow(add(
            size_of::<BatchRow>(),
            add(
                mul(names.len(), size_of::<(String, crate::types::Value)>())?,
                add(name_bytes, lookup_bytes(names.len(), name_bytes)?)?,
            )?,
        )?)?;
        #[cfg(test)]
        crate::executor::execution::graph_memory_test_observer::record(
            "copy_graph_output",
            memory.bytes(),
        );
        let mut values = Vec::with_capacity(names.len());
        for (name, column) in names.iter().zip(&table.columns) {
            let value = match &paths {
                Some(builder) if is_path(&column.expr) => builder.value(env, store, path)?,
                _ => eval.value(&column.expr, entries.get())?,
            };
            let (value, value_memory) = value.into_parts();
            memory.absorb(value_memory);
            values.push((name.clone(), value));
        }
        grow_vec(&mut out, 1, memory)?;
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

    #[test]
    fn should_admit_graph_output_metadata_before_copying() {
        // Arrange
        let (cassie, session) = observer::fixture();
        let _observation = observer::start();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT * FROM GRAPH_TABLE(g MATCH(a IS node WHERE a.id=1)-[IS edge]->(b IS node) \
                 COLUMNS(ARRAY[b.id] AS ids)) AS t",
                vec![],
            )
            .expect("one graph row with array metadata");

        // Assert
        assert_eq!(result.row_count(), 1);
        observer::assert_admitted(
            "before_graph_output_metadata",
            "copy_graph_output_metadata",
            2 * std::mem::size_of::<crate::types::DataType>()
                + std::mem::size_of::<super::BatchRow>()
                + std::mem::size_of::<Vec<super::BatchRow>>(),
        );
    }
}
