//! Brute-force depth-first enumeration of every path matching a pattern.

use super::model::{Graph, Path};
use super::pattern::{Mode, OracleError, Pattern};

/// Whether the whole (possibly partial) `path` obeys `mode`.
///
/// `SIMPLE` lets the first vertex recur only as the final vertex; every other
/// vertex may appear once. `ACYCLIC` forbids any repeated vertex. `TRAIL`
/// forbids repeated edges. `WALK` allows everything.
#[must_use]
pub fn mode_allows(mode: Mode, path: &Path) -> bool {
    let v = &path.vertices;
    match mode {
        Mode::Walk => true,
        Mode::Trail => all_distinct(&path.edges),
        Mode::Acyclic => all_distinct(v),
        Mode::Simple => {
            let last = v.len() - 1;
            all_distinct(&v[1..]) && v[1..last].iter().all(|x| x != &v[0])
        }
    }
}

fn all_distinct<T: PartialEq>(items: &[T]) -> bool {
    items
        .iter()
        .enumerate()
        .all(|(i, a)| items[i + 1..].iter().all(|b| a != b))
}

struct Search<'a> {
    graph: &'a Graph,
    pattern: &'a Pattern,
    out: Vec<Path>,
}

impl Search<'_> {
    /// State: inside edge step `step` having taken `hops` hops from the last
    /// vertex of `path`.
    fn run(&mut self, step: usize, hops: u32, path: &Path) {
        let (edge_pattern, next_vertex) = &self.pattern.steps[step];
        let quantifier = edge_pattern.quantifier;
        let last = self
            .graph
            .vertex(path.end())
            .expect("path vertices exist in the graph");
        // Close this step here. At zero hops the same vertex must satisfy both
        // adjacent declared vertex patterns.
        if hops >= quantifier.min && next_vertex.matches(last) {
            if step + 1 == self.pattern.steps.len() {
                self.out.push(path.clone());
            } else {
                self.run(step + 1, 0, path);
            }
        }
        let may_extend = match quantifier.max {
            Some(max) => hops < max,
            None => true,
        };
        if !may_extend {
            return;
        }
        for hop in self.graph.hops(path.end(), edge_pattern.direction) {
            if !edge_pattern.matches(hop.edge) {
                continue;
            }
            let next = path.extended(&hop);
            if mode_allows(self.pattern.mode, &next) {
                self.run(step, hops + 1, &next);
            }
        }
    }
}

/// Every path matching vertex labels, inline predicates, quantifier bounds
/// and the mode, before any selector or graph-pattern `WHERE`. Sorted by the
/// contract's path order, duplicates (one path reachable through different
/// hop splits of adjacent quantified steps) removed.
///
/// # Errors
///
/// Returns the validation error for an invalid pattern.
pub fn enumerate(graph: &Graph, pattern: &Pattern) -> Result<Vec<Path>, OracleError> {
    pattern.validate()?;
    let mut search = Search {
        graph,
        pattern,
        out: Vec::new(),
    };
    for start in &graph.vertices {
        if !pattern.first.matches(start) {
            continue;
        }
        let path = Path::single(&start.id);
        if pattern.steps.is_empty() {
            search.out.push(path);
        } else {
            search.run(0, 0, &path);
        }
    }
    let mut paths = search.out;
    paths.sort();
    paths.dedup();
    Ok(paths)
}
