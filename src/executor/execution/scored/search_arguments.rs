//! Extraction of the field, query text and syntax of `search`, `search_score`
//! and `snippet` calls for the indexed scored read paths.
//!
//! Every extractor returns `None` for shapes it does not recognize, including
//! an unknown syntax selector, so the call falls back to the generic evaluator
//! that reports the error.

use super::{Expr, FunctionCall, Value};
use crate::search::query::{SearchQueryInput, SearchSyntax};

fn search_syntax_argument(function: &FunctionCall, params: &[Value]) -> Option<SearchSyntax> {
    match function.args.get(2) {
        None => Some(SearchSyntax::Terms),
        Some(Expr::StringLiteral(syntax)) => SearchSyntax::from_argument(syntax).ok(),
        Some(Expr::Param(index)) => params
            .get(*index)
            .and_then(Value::as_str)
            .and_then(|syntax| SearchSyntax::from_argument(syntax).ok()),
        Some(_) => None,
    }
}

fn search_call_args(
    function: &FunctionCall,
    params: &[Value],
) -> Option<(String, SearchQueryInput)> {
    if !(2..=3).contains(&function.args.len()) {
        return None;
    }
    let Expr::Column(field) = &function.args[0] else {
        return None;
    };
    let text = search_query_argument(&function.args[1], params)?;
    let syntax = search_syntax_argument(function, params)?;
    Some((field.clone(), SearchQueryInput::new(text, syntax)))
}

pub(super) fn search_function_args_with_params(
    function: &FunctionCall,
    params: &[Value],
) -> Option<(String, SearchQueryInput)> {
    if !function.name.eq_ignore_ascii_case("search_score") {
        return None;
    }
    search_call_args(function, params)
}

pub(super) fn search_query_function_args(
    function: &FunctionCall,
    name: &str,
    params: &[Value],
) -> Option<(String, SearchQueryInput)> {
    if !function.name.eq_ignore_ascii_case(name) {
        return None;
    }
    search_call_args(function, params)
}

pub(super) fn search_predicate_args_with_params(
    function: &FunctionCall,
    params: &[Value],
) -> Option<(String, SearchQueryInput)> {
    if !matches!(
        function.name.to_ascii_lowercase().as_str(),
        "search" | "search_score"
    ) {
        return None;
    }
    search_call_args(function, params)
}

fn search_query_argument(expr: &Expr, params: &[Value]) -> Option<String> {
    match expr {
        Expr::StringLiteral(query) => Some(query.clone()),
        Expr::Param(index) => params
            .get(*index)
            .and_then(Value::as_str)
            .map(str::to_string),
        _ => None,
    }
}
