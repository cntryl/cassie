//! Window aggregates share grouped argument and FILTER validation.
use super::{coalesce_results::ResultTypes, CassieError};
use crate::sql::ast::{FunctionCall, WindowFunctionCall};

pub(super) fn validate(
    function: &mut WindowFunctionCall,
    types: &ResultTypes,
) -> Result<(), CassieError> {
    if crate::sql::functions::aggregate_window_function(&function.name).is_none() {
        return Ok(());
    }
    let mut aggregate = FunctionCall {
        name: function.name.clone(),
        args: function.args.clone(),
        filter: function.filter.clone(),
        ..FunctionCall::default()
    };
    super::boolean_contexts::validate_boolean_aggregate(&mut aggregate, types)?;
    function.args = aggregate.args;
    function.filter = aggregate.filter;
    Ok(())
}
