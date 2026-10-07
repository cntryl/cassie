//! Preserve the resolved FLOAT domain in the existing variadic COALESCE AST.
//!
//! A typed NULL fallback is a lazy semantic no-op. It retains the common domain
//! when a selected CASE/derived carrier or a bound NULL loses its numeric type.
//! Repeated scope and parameter binding does not add another anchor.
use super::coalesce_results::ResultTypes;
use super::{DataType, Expr, FunctionCall};

pub(super) fn coerce_function(function: &mut FunctionCall, scope: &ResultTypes) {
    if !scope.resolved_coalesce_domains()
        || !function.name.eq_ignore_ascii_case("coalesce")
        || has_float_domain(function)
    {
        return;
    }
    let result_type = function
        .args
        .iter()
        .filter_map(|argument| scope.expression_type(argument))
        .try_fold(DataType::Null, super::inference::common_case_type);
    if result_type == Some(DataType::Float) {
        function.args.push(Expr::Cast {
            expr: Box::new(Expr::Null),
            data_type: DataType::Float,
        });
    }
}

pub(crate) fn has_float_domain(function: &FunctionCall) -> bool {
    function.args.last().is_some_and(|argument| {
        matches!(argument, Expr::Cast { expr, data_type: DataType::Float } if matches!(expr.as_ref(), Expr::Null))
    })
}
