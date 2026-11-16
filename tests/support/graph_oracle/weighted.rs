//! Naive `graph_weighted_paths` oracle: k cheapest SIMPLE paths.

use super::enumerate::mode_allows;
use super::model::{Direction, Edge, ElementRef, Graph, Path, Scalar, SCALE};
use super::pattern::{Mode, OracleError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeightedPath {
    /// One-based rank among the returned rows.
    pub rank: u64,
    /// Total cost in micro-units (`SCALE`).
    pub cost: i128,
    pub path: Path,
    /// One validated weight per traversed edge, path order, micro-units.
    pub edge_costs: Vec<i128>,
}

/// Arguments of `graph_weighted_paths` (the graph is passed separately).
pub struct WeightedQuery<'a> {
    pub source: &'a ElementRef,
    pub target: &'a ElementRef,
    pub weight_property: &'a str,
    pub max_hops: u32,
    pub direction: Direction,
    /// `None` is all labels; `Some(&[])` selects no edge.
    pub edge_labels: Option<&'a [&'a str]>,
    /// Inclusive cost limit in micro-units.
    pub max_cost: Option<i128>,
    pub k: u32,
}

fn weight_of(edge: &Edge, property: &str) -> Result<i128, OracleError> {
    let micro = match edge.props.get(property) {
        Some(Scalar::Num(n)) => *n,
        Some(Scalar::Int(i)) => i128::from(*i) * SCALE,
        _ => return Err(OracleError::InvalidWeight),
    };
    if micro < 0 {
        return Err(OracleError::InvalidWeight);
    }
    Ok(micro)
}

fn selected(edge: &Edge, labels: Option<&[&str]>) -> bool {
    labels.is_none_or(|ls| ls.iter().any(|l| edge.labels.contains(*l)))
}

struct Walker<'a> {
    graph: &'a Graph,
    query: &'a WeightedQuery<'a>,
    found: Vec<WeightedPath>,
}

impl Walker<'_> {
    fn run(&mut self, path: &Path, costs: &[i128], total: i128) -> Result<(), OracleError> {
        if path.end() == self.query.target && self.query.max_cost.is_none_or(|m| total <= m) {
            self.found.push(WeightedPath {
                rank: 0,
                cost: total,
                path: path.clone(),
                edge_costs: costs.to_vec(),
            });
        }
        if path.hop_count() >= self.query.max_hops as usize {
            return Ok(());
        }
        for hop in self.graph.hops(path.end(), self.query.direction) {
            if !selected(hop.edge, self.query.edge_labels) {
                continue;
            }
            let next = path.extended(&hop);
            if !mode_allows(Mode::Simple, &next) {
                continue;
            }
            let weight = weight_of(hop.edge, self.query.weight_property)?;
            let sum = total.checked_add(weight).ok_or(OracleError::Overflow)?;
            let mut next_costs = costs.to_vec();
            next_costs.push(weight);
            self.run(&next, &next_costs, sum)?;
        }
        Ok(())
    }
}

/// The `k` cheapest SIMPLE paths from source to target, ordered by cost and
/// then by the contract's path order.
///
/// Before anything else every graph-visible edge in the selected edge
/// mappings (here: live edges passing the label selection) must carry a
/// valid weight, whether or not any returned path uses it.
///
/// # Errors
///
/// `InvalidWeight`, `ZeroK`, `UnknownVertex` or `Overflow`.
pub fn weighted_paths(
    graph: &Graph,
    query: &WeightedQuery<'_>,
) -> Result<Vec<WeightedPath>, OracleError> {
    if query.k == 0 {
        return Err(OracleError::ZeroK);
    }
    if graph.vertex(query.source).is_none() || graph.vertex(query.target).is_none() {
        return Err(OracleError::UnknownVertex);
    }
    for edge in graph.live_edges() {
        if selected(edge, query.edge_labels) {
            weight_of(edge, query.weight_property)?;
        }
    }
    let mut walker = Walker {
        graph,
        query,
        found: Vec::new(),
    };
    walker.run(&Path::single(query.source), &[], 0)?;
    let mut rows = walker.found;
    rows.sort_by(|a, b| a.cost.cmp(&b.cost).then_with(|| a.path.cmp(&b.path)));
    rows.truncate(query.k as usize);
    for (i, row) in rows.iter_mut().enumerate() {
        row.rank = u64::try_from(i).expect("rank fits") + 1;
    }
    Ok(rows)
}
