//! Entry point for a bound `GRAPH_TABLE` source (#1257); the work lives in `graph_table/`.

#[path = "graph_table/mod.rs"]
mod graph_table;

use super::{finalize_source_batches, BatchRow, QuerySource, SourceExecution, SourceExecutionEnv};

pub(super) fn execute(
    env: &SourceExecutionEnv<'_>,
    source: &QuerySource,
    outer_row: Option<&BatchRow>,
    qualify: bool,
) -> SourceExecution {
    graph_table::execute(env, source, outer_row, qualify)
}
