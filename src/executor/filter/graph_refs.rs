//! `cassie_graph.vertex_ref` and `cassie_graph.edge_ref` (#1268).
use super::{EvalContext, QueryError, Value};
use crate::sql::functions::FunctionId;

pub(super) fn evaluate(
    _id: FunctionId,
    _args: &[Value],
    _context: EvalContext<'_>,
) -> Result<Value, QueryError> {
    Err(super::graph_functions::failure(
        "0A000",
        "graph element reference resolution is not available",
    ))
}
