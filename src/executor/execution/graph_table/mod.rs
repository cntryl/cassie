//! Exact execution of a bound `GRAPH_TABLE` source (#1257).
//!
//! Every table the pattern reads is scanned through the statement's ordinary read path, so
//! the statement sees exactly what a scan of those tables sees. Adjacency is built per
//! statement over the visible rows, paths are enumerated exhaustively under the pattern's
//! mode, the selector ranks them by the documented total order, and the pattern `WHERE`
//! and `COLUMNS` run on the selected paths. The complete result is materialized before any
//! row leaves the source.

mod bindings;
mod eval;
mod keys;
mod plan;
mod project;
mod select;
mod store;
mod walk;

use std::sync::Arc;

use super::super::{batch, source_collection, BatchRow, QueryError, SourceExecutionEnv};
use super::{finalize_source_batches, SourceExecution};
use crate::sql::ast::{QuerySource, TableFunctionForm};
use crate::types::DataType;

fn failure(message: &str) -> QueryError {
    QueryError::Cassie(crate::app::CassieError::Execution(message.to_owned()))
}

/// Runs the `GRAPH_TABLE` source `source`; `outer_row` supplies the correlated columns of a
/// `LATERAL` source.
pub(super) fn execute(
    env: &SourceExecutionEnv<'_>,
    source: &QuerySource,
    outer_row: Option<&BatchRow>,
    qualify: bool,
) -> SourceExecution {
    let QuerySource::TableFunction {
        name,
        form: Some(form),
        lateral,
        ..
    } = source
    else {
        return Err(failure("GRAPH_TABLE source has no pattern"));
    };
    let TableFunctionForm::GraphTable(table) = form.as_ref() else {
        return Err(failure("GRAPH_TABLE source has no pattern"));
    };
    env.controls.check_interruption()?;
    let binding = table
        .binding
        .as_ref()
        .ok_or_else(|| failure("GRAPH_TABLE source was not bound"))?;
    let graph = env
        .cassie
        .catalog
        .get_property_graph(&binding.graph)
        .ok_or_else(|| failure("the property graph of a GRAPH_TABLE no longer exists"))?;
    let plan = plan::Plan::build(table, graph)?;
    let eval = eval::Evaluator::new(env, if *lateral { outer_row } else { None });
    let mut output = env.controls.reserve_query_memory(0)?;
    let rows = {
        let mut traversal = env.controls.reserve_query_memory(0)?;
        let store = store::Store::load(env, &plan, &mut traversal)?;
        let found = walk::Walk::new(env, &plan, &store, &eval, &mut traversal).run()?;
        let kind = table.effective_selector().kind;
        let selected = select::apply(env, &store, kind, found)?;
        project::rows(env, &plan, &store, &eval, &selected, &mut output)?
    };
    env.controls.check_interruption()?;
    let types = Arc::new(binding.output.clone());
    let lease = Arc::new(output);
    let rows = rows
        .into_iter()
        .map(|mut row| {
            row.set_data_types(Arc::clone(&types));
            row.with_query_memory(Some(Arc::clone(&lease)))
        })
        .collect::<Vec<_>>();
    let text_fields = binding
        .output
        .iter()
        .enumerate()
        .filter(|(_, data_type)| **data_type == DataType::Text)
        .map(|(index, _)| crate::sql::table_sources::physical_name(index))
        .collect();
    let batches = batch::chunk_rows(rows, batch::DEFAULT_BATCH_SIZE);
    finalize_source_batches(env, batches, text_fields, qualify, name)
}
