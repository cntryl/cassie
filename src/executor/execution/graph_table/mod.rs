//! Exact execution of a bound `GRAPH_TABLE` source (#1257).
//!
//! Every table the pattern reads is scanned through the statement's ordinary read path, so
//! the statement sees exactly what a scan of those tables sees. Adjacency is built per
//! statement over the visible rows, paths are enumerated exhaustively under the pattern's
//! mode, the selector ranks them by the documented total order, and the pattern `WHERE`
//! and `COLUMNS` run on the selected paths. The complete result is materialized before any
//! row leaves the source.

mod access;
mod bindings;
mod cache;
mod eval;
mod keys;
mod lazy;
mod lazy_rows;
mod path_value;
mod plan;
mod project;
mod select;
mod store;
mod walk;
mod weighted;
mod weighted_args;
mod weighted_run;
mod weighted_search;
mod weighted_validate;
mod weighted_weight;

pub(super) use weighted::execute as execute_weighted;

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
    let eval = eval::Evaluator::new(env, if *lateral { outer_row } else { None })?;
    let mut output = env.controls.reserve_query_memory(0)?;
    let rows = {
        let mut traversal = env.controls.reserve_query_memory(0)?;
        let access = access::Access::choose(env.cassie, env.session, &plan);
        let mut store = if access.forced_exact {
            store::Store::load(env, &plan, &mut traversal)?
        } else {
            store::Store::lazy(env, &plan.graph, access)?
        };
        let found = walk::Walk::new(env, &plan, &mut store, &eval, &mut traversal).run()?;
        let kind = table.effective_selector().kind;
        let selected = select::apply(env, &store, kind, found)?;
        let rows = project::rows(env, &plan, &store, &eval, &selected, &mut output)?;
        store.record_reads(env, 0);
        rows
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

/// The `EXPLAIN` description of every `GRAPH_TABLE` source reachable from `source`: its
/// graph and the access path of each mapping.
pub(crate) fn explain_sources(
    cassie: &crate::app::Cassie,
    session: Option<&crate::app::CassieSession>,
    source: &QuerySource,
    out: &mut Vec<String>,
) {
    match source {
        QuerySource::Aliased { source, .. } | QuerySource::TableSample { source, .. } => {
            explain_sources(cassie, session, source, out);
        }
        QuerySource::Join { left, right, .. } => {
            explain_sources(cassie, session, left, out);
            explain_sources(cassie, session, right, out);
        }
        QuerySource::Subquery { select, .. } => {
            explain_sources(cassie, session, &select.source, out);
        }
        QuerySource::TableFunction {
            form: Some(form), ..
        } => {
            let TableFunctionForm::GraphTable(table) = form.as_ref() else {
                return;
            };
            let Some(graph) = table
                .binding
                .as_ref()
                .and_then(|binding| cassie.catalog.get_property_graph(&binding.graph))
            else {
                return;
            };
            let name = graph.name.clone();
            let Ok(plan) = plan::Plan::build(table, graph) else {
                return;
            };
            let access = access::Access::choose(cassie, session, &plan);
            out.push(format!(
                "graph_table={name} {}",
                access::describe(&access, &plan, &plan.graph)
            ));
        }
        _ => {}
    }
}
