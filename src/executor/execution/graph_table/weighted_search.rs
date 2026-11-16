//! Exhaustive search for the `k` cheapest SIMPLE paths of `graph_weighted_paths` (#1258).
//!
//! A depth-first walk with an explicit stack visits every simple path of at most `max_hops`
//! edges from the source. Nothing is pruned: a candidate that cannot enter the retained
//! `k` is only not kept, so the answer is the first `k` of all qualifying paths in
//! `(cost, path order)` order. A SIMPLE path repeats no vertex, except that it may end on
//! its own start, and such a closing path is never extended. The stack, the retained
//! candidates and the per-edge state are accounted against the query budget, and
//! cancellation is checked on every step.

use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::mem::size_of;

use uuid::Uuid;

use super::store::Store;
use super::weighted_args::{diagnostic, Way};
use super::{QueryError, SourceExecutionEnv};
use crate::executor::retained_memory::{add, mul};
use crate::runtime::QueryMemoryReservation;
use crate::types::extended::numeric::{math, Numeric};
use crate::types::graph::{Direction, ElementKind, ElementRef, KeyPart, Path};

/// Heap held by one `Numeric` beyond its inline size, as accounted.
const NUMERIC_HEAP: usize = 64;

/// One qualifying path with its cost and per-edge costs.
pub(super) struct Candidate {
    pub(super) cost: Numeric,
    pub(super) path: Path,
    pub(super) edge_costs: Vec<Numeric>,
}

impl Candidate {
    fn bytes(&self) -> Result<usize, QueryError> {
        let elements = self.path.vertices.len() + self.path.edges.len();
        let keys = self
            .path
            .vertices
            .iter()
            .chain(&self.path.edges)
            .map(|element| {
                element.key.len() * size_of::<KeyPart>()
                    + element
                        .key
                        .iter()
                        .map(|part| match part {
                            KeyPart::Text(text) => text.capacity(),
                            _ => 0,
                        })
                        .sum::<usize>()
            })
            .sum::<usize>();
        let costs = mul(
            self.edge_costs.len() + 1,
            size_of::<Numeric>() + NUMERIC_HEAP,
        )?;
        let shape = add(
            mul(elements, size_of::<ElementRef>())?,
            self.path.directions.len(),
        )?;
        Ok(add(add(keys, costs)?, add(shape, size_of::<Self>())?)?)
    }
}

impl PartialEq for Candidate {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Candidate {}

impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Cost first, then the contract's total path order.
impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> Ordering {
        self.cost
            .compare(&other.cost)
            .then_with(|| self.path.cmp(&other.path))
    }
}

/// What a search needs besides the store.
pub(super) struct Query<'a> {
    pub(super) source: usize,
    pub(super) target: usize,
    pub(super) way: Way,
    pub(super) max_hops: usize,
    pub(super) max_cost: Option<&'a Numeric>,
    pub(super) k: usize,
    /// Cost of each edge, by edge id.
    pub(super) weights: &'a [Numeric],
    /// Durable identity of each vertex and edge mapping.
    pub(super) vertex_mappings: &'a [Uuid],
    pub(super) edge_mappings: &'a [Uuid],
    pub(super) database_id: Uuid,
    pub(super) graph_id: Uuid,
}

struct Hop {
    edge: usize,
    direction: Direction,
    to: usize,
}

struct Frame {
    vertex: usize,
    cursor: usize,
}

pub(super) struct Search<'a, 'e> {
    env: &'a SourceExecutionEnv<'e>,
    store: &'a Store,
    query: &'a Query<'a>,
    memory: &'a mut QueryMemoryReservation,
    on_path: Vec<bool>,
    vertices: Vec<usize>,
    edges: Vec<usize>,
    directions: Vec<Direction>,
    totals: Vec<Numeric>,
    best: BinaryHeap<Candidate>,
}

fn key_part(part: &super::keys::KeyPart) -> KeyPart {
    match part {
        super::keys::KeyPart::SmallInt(value) => KeyPart::SmallInt(*value),
        super::keys::KeyPart::Integer(value) => KeyPart::Int(*value),
        super::keys::KeyPart::BigInt(value) => KeyPart::BigInt(*value),
        super::keys::KeyPart::Text(value) => KeyPart::Text(value.clone()),
        super::keys::KeyPart::Uuid(bytes) => KeyPart::Uuid(Uuid::from_bytes(*bytes)),
    }
}

impl<'a, 'e> Search<'a, 'e> {
    pub(super) fn new(
        env: &'a SourceExecutionEnv<'e>,
        store: &'a Store,
        query: &'a Query<'a>,
        memory: &'a mut QueryMemoryReservation,
    ) -> Result<Self, QueryError> {
        memory.try_grow(add(
            store.vertices.len(),
            mul(
                query.max_hops.min(store.vertices.len()) + 2,
                4 * size_of::<usize>() + size_of::<Numeric>() + NUMERIC_HEAP,
            )?,
        )?)?;
        Ok(Self {
            env,
            store,
            query,
            memory,
            on_path: vec![false; store.vertices.len()],
            vertices: Vec::new(),
            directions: Vec::new(),
            edges: Vec::new(),
            totals: Vec::new(),
            best: BinaryHeap::new(),
        })
    }

    fn element(
        &self,
        kind: ElementKind,
        mapping: Uuid,
        key: &[super::keys::KeyPart],
    ) -> ElementRef {
        ElementRef {
            database_id: self.query.database_id,
            graph_id: self.query.graph_id,
            mapping_id: mapping,
            kind,
            key: key.iter().map(key_part).collect(),
        }
    }

    fn vertex_ref(&self, id: usize) -> ElementRef {
        let (mapping, row) = self.store.vertices[id];
        let loaded = self.store.vertex_maps[mapping].as_ref().expect("loaded");
        self.element(
            ElementKind::Vertex,
            self.query.vertex_mappings[mapping],
            &loaded.keys[row],
        )
    }

    fn edge_ref(&self, id: usize) -> ElementRef {
        let record = self.store.edges[id];
        let loaded = self.store.edge_maps[record.mapping]
            .as_ref()
            .expect("loaded");
        self.element(
            ElementKind::Edge,
            self.query.edge_mappings[record.mapping],
            &loaded.keys[record.row],
        )
    }

    /// The next traversal choice leaving `vertex`: outgoing edges first, then incoming.
    fn next_hop(&self, vertex: usize, cursor: &mut usize) -> Option<Hop> {
        let outgoing = if self.query.way == Way::In {
            &[][..]
        } else {
            &self.store.outgoing[vertex][..]
        };
        if *cursor < outgoing.len() {
            let edge = outgoing[*cursor];
            *cursor += 1;
            return Some(Hop {
                edge,
                direction: Direction::Forward,
                to: self.store.edges[edge].destination,
            });
        }
        if self.query.way == Way::Out {
            return None;
        }
        let incoming = &self.store.incoming[vertex];
        while let Some(&edge) = incoming.get(*cursor - outgoing.len()) {
            *cursor += 1;
            let record = self.store.edges[edge];
            // A self-loop is one step under `both`, taken in its stored direction.
            if self.query.way == Way::Both && record.source == record.destination {
                continue;
            }
            return Some(Hop {
                edge,
                direction: Direction::Reverse,
                to: record.source,
            });
        }
        None
    }

    /// Keeps the path ending with `hop` (or the zero-hop path) when it can be among the `k`
    /// best.
    fn offer(&mut self, cost: &Numeric, hop: Option<&Hop>) -> Result<(), QueryError> {
        if self
            .query
            .max_cost
            .is_some_and(|limit| cost.compare(limit) == Ordering::Greater)
        {
            return Ok(());
        }
        let full = self.best.len() >= self.query.k;
        if full
            && self
                .best
                .peek()
                .is_some_and(|worst| cost.compare(&worst.cost) == Ordering::Greater)
        {
            return Ok(());
        }
        let mut vertices = self
            .vertices
            .iter()
            .map(|id| self.vertex_ref(*id))
            .collect::<Vec<_>>();
        let mut edges = self
            .edges
            .iter()
            .map(|id| self.edge_ref(*id))
            .collect::<Vec<_>>();
        let mut directions = self.directions.clone();
        let mut edge_costs = self
            .edges
            .iter()
            .map(|id| self.query.weights[*id].clone())
            .collect::<Vec<_>>();
        if let Some(hop) = hop {
            vertices.push(self.vertex_ref(hop.to));
            edges.push(self.edge_ref(hop.edge));
            directions.push(hop.direction);
            edge_costs.push(self.query.weights[hop.edge].clone());
        }
        let candidate = Candidate {
            cost: cost.clone(),
            path: Path {
                vertices,
                edges,
                directions,
            },
            edge_costs,
        };
        if full && self.best.peek().is_some_and(|worst| candidate >= *worst) {
            return Ok(());
        }
        let bytes = candidate.bytes()?;
        self.memory.try_grow(bytes)?;
        self.best.push(candidate);
        if self.best.len() > self.query.k {
            if let Some(evicted) = self.best.pop() {
                let held = self.memory.bytes();
                self.memory.shrink_to(held.saturating_sub(evicted.bytes()?));
            }
        }
        Ok(())
    }

    fn add_cost(&self, edge: usize) -> Result<Numeric, QueryError> {
        let total = self.totals.last().expect("a path has a cost");
        math::add(total, &self.query.weights[edge])
            .map_err(|error| diagnostic(error.sqlstate, error.message))
    }

    /// Runs the search and returns the retained candidates, cheapest first.
    pub(super) fn run(mut self) -> Result<Vec<Candidate>, QueryError> {
        let source = self.query.source;
        self.vertices.push(source);
        self.totals.push(Numeric::zero());
        self.on_path[source] = true;
        if source == self.query.target {
            self.offer(&Numeric::zero(), None)?;
        }
        // The retained prefix holds the vertices before the one being expanded.
        let mut stack = vec![Frame {
            vertex: source,
            cursor: 0,
        }];
        while let Some(top) = stack.len().checked_sub(1) {
            self.env.controls.check_interruption()?;
            let hops = top;
            let vertex = stack[top].vertex;
            let next = if hops >= self.query.max_hops {
                None
            } else {
                let mut cursor = stack[top].cursor;
                let hop = self.next_hop(vertex, &mut cursor);
                stack[top].cursor = cursor;
                hop
            };
            let Some(hop) = next else {
                stack.pop();
                if top > 0 {
                    // Leave the vertex this frame entered by.
                    if let Some(left) = self.vertices.pop() {
                        self.on_path[left] = false;
                    }
                    self.edges.pop();
                    self.directions.pop();
                    self.totals.pop();
                }
                continue;
            };
            let closing = hop.to == source;
            if self.on_path[hop.to] && !closing {
                continue;
            }
            let cost = self.add_cost(hop.edge)?;
            if hop.to == self.query.target {
                self.offer(&cost, Some(&hop))?;
            }
            if closing {
                continue;
            }
            self.vertices.push(hop.to);
            self.edges.push(hop.edge);
            self.directions.push(hop.direction);
            self.totals.push(cost);
            self.on_path[hop.to] = true;
            stack.push(Frame {
                vertex: hop.to,
                cursor: 0,
            });
        }
        Ok(self.best.into_sorted_vec())
    }
}
