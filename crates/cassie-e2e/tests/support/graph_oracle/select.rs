//! Selectors (`ALL`, `ANY SHORTEST`, `SHORTEST k`) and the graph-pattern
//! `WHERE`, applied after selection.

use std::collections::BTreeMap;

use super::enumerate::enumerate;
use super::model::{ElementRef, Graph, Path};
use super::pattern::{OracleError, Pattern, Selector};

/// Applies a selector. Shortest selectors rank by hop count, then by the
/// contract's path order, separately for each (start, end) endpoint pair.
#[must_use]
pub fn select(paths: Vec<Path>, selector: Selector) -> Vec<Path> {
    let limit = match selector {
        Selector::All => return paths,
        Selector::AnyShortest => 1,
        Selector::Shortest(k) => k as usize,
    };
    let mut by_pair: BTreeMap<(ElementRef, ElementRef), Vec<Path>> = BTreeMap::new();
    for path in paths {
        by_pair
            .entry((path.start().clone(), path.end().clone()))
            .or_default()
            .push(path);
    }
    let mut chosen = Vec::new();
    for (_, mut group) in by_pair {
        group.sort_by(|a, b| a.hop_count().cmp(&b.hop_count()).then_with(|| a.cmp(b)));
        chosen.extend(group.into_iter().take(limit));
    }
    chosen.sort();
    chosen
}

/// Full pipeline: enumerate, select, then apply the graph-pattern `WHERE`
/// to the selected paths. Result sorted by path order.
///
/// # Errors
///
/// Returns the validation error for an invalid pattern.
pub fn match_paths(graph: &Graph, pattern: &Pattern) -> Result<Vec<Path>, OracleError> {
    let enumerated = super::enumerate::enumerate(graph, pattern)?;
    let mut selected = select(enumerated, pattern.selector);
    if let Some(filter) = &pattern.post_filter {
        selected.retain(|path| filter(graph, path));
    }
    Ok(selected)
}

/// Enumeration only, for callers comparing pre-selection candidates.
///
/// # Errors
///
/// Returns the validation error for an invalid pattern.
pub fn candidates(graph: &Graph, pattern: &Pattern) -> Result<Vec<Path>, OracleError> {
    enumerate(graph, pattern)
}
