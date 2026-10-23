//! Window aggregate folds reuse the row aggregate transition rules.

use std::collections::HashMap;

use crate::app::CassieSession;
use crate::catalog::FunctionMeta;
use crate::executor::batch::BatchRow;
use crate::executor::filter;
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::FunctionCall;
use crate::types::Value;

use super::state::AggregateAccumulator;
use super::QueryError;

pub(super) fn evaluate<'a>(
    function: &FunctionCall,
    rows: impl IntoIterator<Item = &'a BatchRow>,
    params: &[Value],
    search_context: Option<&filter::SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
    controls: &QueryExecutionControls,
) -> Result<Value, QueryError> {
    let mut accumulator = AggregateAccumulator::new(function);
    let _memory = controls.reserve_query_memory(accumulator.retained_bytes())?;
    for row in rows {
        super::super::check_timeout(controls)?;
        accumulator.update(
            function,
            row,
            params,
            search_context,
            user_functions,
            session,
        )?;
    }
    accumulator.finish()
}
