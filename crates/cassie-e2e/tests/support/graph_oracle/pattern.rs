//! Pattern description: steps, quantifiers, modes, selectors and errors.

use std::collections::BTreeSet;

use super::model::{Direction, Edge, Graph, Path, Vertex};

pub type VertexPredicate = Box<dyn Fn(&Vertex) -> bool>;
pub type EdgePredicate = Box<dyn Fn(&Edge) -> bool>;
pub type PathPredicate = Box<dyn Fn(&Graph, &Path) -> bool>;

/// Oracle failures, with the SQLSTATE the contract assigns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OracleError {
    /// Unbounded `WALK` is rejected (even with a selector).
    UnboundedWalk,
    /// `m > n` in a quantifier (`22023`).
    InvalidBounds,
    /// `k = 0` (`22023`).
    ZeroK,
    /// Missing, NULL, negative, non-numeric or non-finite weight (`22023`).
    InvalidWeight,
    /// Source or target is not a graph vertex (`42704`).
    UnknownVertex,
    /// Cost sum is unrepresentable (`22003`).
    Overflow,
}

fn labels_match(wanted: &[String], have: &BTreeSet<String>) -> bool {
    // A set test: an element carrying several wanted labels matches once.
    wanted.is_empty() || wanted.iter().any(|l| have.contains(l))
}

#[derive(Default)]
pub struct VertexPattern {
    /// Label alternatives (`IS a|b`); empty means no label constraint.
    pub labels: Vec<String>,
    pub predicate: Option<VertexPredicate>,
}

impl VertexPattern {
    #[must_use]
    pub fn any() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn labeled(labels: &[&str]) -> Self {
        Self {
            labels: labels.iter().map(|l| (*l).to_string()).collect(),
            predicate: None,
        }
    }

    #[must_use]
    pub fn when(mut self, predicate: impl Fn(&Vertex) -> bool + 'static) -> Self {
        self.predicate = Some(Box::new(predicate));
        self
    }

    #[must_use]
    pub fn matches(&self, vertex: &Vertex) -> bool {
        labels_match(&self.labels, &vertex.labels)
            && self.predicate.as_ref().is_none_or(|p| p(vertex))
    }
}

/// Inclusive hop bounds; `max = None` is unbounded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Quantifier {
    pub min: u32,
    pub max: Option<u32>,
}

impl Quantifier {
    /// Absent quantifier: exactly one edge.
    #[must_use]
    pub fn one() -> Self {
        Self::exactly(1)
    }

    /// `{n}`.
    #[must_use]
    pub fn exactly(n: u32) -> Self {
        Self::between(n, n)
    }

    /// `{m,n}`.
    #[must_use]
    pub fn between(m: u32, n: u32) -> Self {
        Self {
            min: m,
            max: Some(n),
        }
    }

    /// `*`: zero or more.
    #[must_use]
    pub fn star() -> Self {
        Self { min: 0, max: None }
    }

    /// `+`: one or more.
    #[must_use]
    pub fn plus() -> Self {
        Self { min: 1, max: None }
    }
}

pub struct EdgePattern {
    pub labels: Vec<String>,
    pub predicate: Option<EdgePredicate>,
    pub direction: Direction,
    pub quantifier: Quantifier,
}

impl EdgePattern {
    #[must_use]
    pub fn new(direction: Direction) -> Self {
        Self {
            labels: Vec::new(),
            predicate: None,
            direction,
            quantifier: Quantifier::one(),
        }
    }

    #[must_use]
    pub fn labeled(mut self, labels: &[&str]) -> Self {
        self.labels = labels.iter().map(|l| (*l).to_string()).collect();
        self
    }

    /// Inline `WHERE`: evaluated per edge before any selection.
    #[must_use]
    pub fn when(mut self, predicate: impl Fn(&Edge) -> bool + 'static) -> Self {
        self.predicate = Some(Box::new(predicate));
        self
    }

    #[must_use]
    pub fn quantified(mut self, quantifier: Quantifier) -> Self {
        self.quantifier = quantifier;
        self
    }

    #[must_use]
    pub fn matches(&self, edge: &Edge) -> bool {
        labels_match(&self.labels, &edge.labels) && self.predicate.as_ref().is_none_or(|p| p(edge))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Walk,
    Trail,
    Acyclic,
    Simple,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selector {
    /// `ALL [mode] PATHS` (the default).
    All,
    /// `ANY SHORTEST [mode] PATH`.
    AnyShortest,
    /// `SHORTEST k [mode] PATHS`.
    Shortest(u32),
}

/// `vertex {edge [quantifier] vertex}` plus selector, mode and the
/// graph-pattern `WHERE` (applied to selected complete paths).
pub struct Pattern {
    pub first: VertexPattern,
    pub steps: Vec<(EdgePattern, VertexPattern)>,
    pub mode: Mode,
    pub selector: Selector,
    pub post_filter: Option<PathPredicate>,
}

impl Pattern {
    #[must_use]
    pub fn new(first: VertexPattern) -> Self {
        Self {
            first,
            steps: Vec::new(),
            mode: Mode::Walk,
            selector: Selector::All,
            post_filter: None,
        }
    }

    #[must_use]
    pub fn step(mut self, edge: EdgePattern, vertex: VertexPattern) -> Self {
        self.steps.push((edge, vertex));
        self
    }

    #[must_use]
    pub fn mode(mut self, mode: Mode) -> Self {
        self.mode = mode;
        self
    }

    #[must_use]
    pub fn select(mut self, selector: Selector) -> Self {
        self.selector = selector;
        self
    }

    #[must_use]
    pub fn keep_where(mut self, filter: impl Fn(&Graph, &Path) -> bool + 'static) -> Self {
        self.post_filter = Some(Box::new(filter));
        self
    }

    /// Static validation: bounds, `k > 0` and the unbounded-`WALK` rejection.
    ///
    /// # Errors
    ///
    /// Returns the matching `OracleError` for an invalid pattern.
    pub fn validate(&self) -> Result<(), OracleError> {
        if self.selector == Selector::Shortest(0) {
            return Err(OracleError::ZeroK);
        }
        for (edge, _) in &self.steps {
            let q = edge.quantifier;
            if q.max.is_some_and(|max| q.min > max) {
                return Err(OracleError::InvalidBounds);
            }
            if q.max.is_none() && self.mode == Mode::Walk {
                return Err(OracleError::UnboundedWalk);
            }
        }
        Ok(())
    }
}
