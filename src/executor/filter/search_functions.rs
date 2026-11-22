//! Row-wise (authoritative) evaluation of `search`, `search_score` and
//! `snippet`.

use super::functions::{require_arg_count_range, to_text};
use super::search::{simple_search_score, SearchContext, SearchTermStats};
use super::search_prepared::query_snippet;
use super::{AnalyzerConfig, Expr, FunctionCall, QueryError, Value};
use crate::runtime::QueryExecutionControls;
use crate::search::query::{SearchQueryInput, SearchSyntax};

pub(super) fn evaluate(
    name: &str,
    function: &FunctionCall,
    args: &[Value],
    search_context: Option<&SearchContext>,
    controls: Option<&QueryExecutionControls>,
) -> Option<Result<Value, QueryError>> {
    match name {
        "search" | "search_score" => Some(evaluate_search_score(
            name,
            function,
            args,
            search_context,
            controls,
        )),
        "snippet" => Some(evaluate_snippet(function, args, search_context, controls)),
        _ => None,
    }
}

fn syntax_argument(name: &str, args: &[Value]) -> Result<SearchSyntax, QueryError> {
    require_arg_count_range(name, args, 2..=3)?;
    match args.get(2) {
        None => Ok(SearchSyntax::Terms),
        Some(Value::String(syntax)) => Ok(SearchSyntax::from_argument(syntax)?),
        Some(_) => Err(QueryError::Cassie(
            crate::app::CassieError::InvalidParameterValue(format!(
                "{name} expects its syntax argument to be 'terms' or 'query'"
            )),
        )),
    }
}

fn source_field(function: &FunctionCall) -> Option<&str> {
    match function.args.first() {
        Some(Expr::Column(field)) => Some(field.as_str()),
        _ => None,
    }
}

fn analyzer_for(search_context: Option<&SearchContext>, field: Option<&str>) -> AnalyzerConfig {
    match (search_context, field) {
        (Some(context), Some(field)) => context.analyzer_for_field(field),
        _ => AnalyzerConfig::default(),
    }
}

fn evaluate_search_score(
    name: &str,
    function: &FunctionCall,
    args: &[Value],
    search_context: Option<&SearchContext>,
    controls: Option<&QueryExecutionControls>,
) -> Result<Value, QueryError> {
    let syntax = syntax_argument(name, args)?;
    let source = to_text(&args[0]);
    let query = to_text(&args[1]);
    let field = source_field(function);
    let (matched, score) = match syntax {
        SearchSyntax::Terms => match search_context {
            Some(context) if field.is_none() && context.total_documents() > 0 => {
                context.match_and_score_text(None, &source, &query)
            }
            Some(context) => context.match_and_score_text(field, &source, &query),
            None => {
                let score = simple_search_score(&source, &query);
                (score > 0.0, score)
            }
        },
        SearchSyntax::Query => {
            let score = query_syntax_score(search_context, field, &source, &query, controls)?;
            (score.is_some(), score.unwrap_or(0.0))
        }
    };
    if name.eq_ignore_ascii_case("search") {
        Ok(Value::Bool(matched))
    } else {
        Ok(Value::Float64(score))
    }
}

/// The BM25 score of `source` for `'query'`-syntax `query`, or `None` when the
/// row does not satisfy it.
fn query_syntax_score(
    search_context: Option<&SearchContext>,
    field: Option<&str>,
    source: &str,
    query: &str,
    controls: Option<&QueryExecutionControls>,
) -> Result<Option<f64>, QueryError> {
    let input = SearchQueryInput::new(query, SearchSyntax::Query);
    let analyzer = analyzer_for(search_context, field);
    let stats = SearchTermStats::from_text_with_positions(Some(source), &analyzer);
    let resolved = SearchContext::resolved_query(search_context, field, &input, &stats)?;
    let _memory = controls
        .map(|controls| controls.reserve_query_memory(resolved.estimated_bytes()))
        .transpose()?;
    let Some(terms) = resolved.matched_terms(&stats) else {
        return Ok(None);
    };
    Ok(Some(match search_context {
        Some(context) => context.score_term_stats(field, &stats, &terms),
        None => SearchContext::default().score_term_stats(field, &stats, &terms),
    }))
}

fn evaluate_snippet(
    function: &FunctionCall,
    args: &[Value],
    search_context: Option<&SearchContext>,
    controls: Option<&QueryExecutionControls>,
) -> Result<Value, QueryError> {
    let syntax = syntax_argument("snippet", args)?;
    let source = to_text(&args[0]);
    let query = to_text(&args[1]);
    let field = source_field(function);
    let analyzer = analyzer_for(search_context, field);
    match syntax {
        SearchSyntax::Terms => {
            let terms = analyzer.analyze(&query);
            Ok(Value::String(crate::search::snippet(
                &source, &terms, &analyzer,
            )))
        }
        SearchSyntax::Query => {
            let input = SearchQueryInput::new(query, SearchSyntax::Query);
            let stats = SearchTermStats::from_text_with_positions(Some(&source), &analyzer);
            let resolved = SearchContext::resolved_query(search_context, field, &input, &stats)?;
            let _memory = controls
                .map(|controls| controls.reserve_query_memory(resolved.estimated_bytes()))
                .transpose()?;
            Ok(Value::String(query_snippet(&resolved, &source, &analyzer)))
        }
    }
}
