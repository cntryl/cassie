//! Entry point for a bound `GRAPH_TABLE` source (#1257); the work lives in `graph_table/`.

#[path = "graph_table/mod.rs"]
mod graph_table;

use super::{finalize_source_batches, BatchRow, QuerySource, SourceExecution, SourceExecutionEnv};
pub(crate) use graph_table::explain_sources as explain_graph_sources;

pub(super) fn execute(
    env: &SourceExecutionEnv<'_>,
    source: &QuerySource,
    outer_row: Option<&BatchRow>,
    qualify: bool,
) -> SourceExecution {
    graph_table::execute(env, source, outer_row, qualify)
}

pub(super) fn execute_weighted(
    env: &SourceExecutionEnv<'_>,
    name: &str,
    function: &crate::sql::ast::FunctionCall,
    outer_row: Option<&BatchRow>,
    qualify: bool,
) -> SourceExecution {
    graph_table::execute_weighted(env, name, function, outer_row, qualify)
}
