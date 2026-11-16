//! Exhaustive enumeration of the paths matching a pattern under its mode.
//!
//! The search is depth-first over an explicit stack, so a long finite bound cannot exhaust
//! the thread stack. Nothing is pruned except by the rules of the pattern: label and
//! inline-predicate mismatches, the mode's repetition rule and the quantifier bounds.
//! Every visited path state, frame and retained candidate is accounted against the query
//! memory budget, and cancellation is checked on each iteration.

use std::collections::{HashMap, HashSet};
use std::mem::size_of;

use super::bindings::{group_entries, scalar_entries, Candidate};
use super::eval::{element_entries, Evaluator};
use super::plan::{Plan, Source};
use super::store::Store;
use super::{QueryError, SourceExecutionEnv};
use crate::runtime::QueryMemoryReservation;
use crate::sql::ast::{GraphEdgeDirection, GraphPathMode};

struct Hop {
    edge: usize,
    direction: u8,
    to: usize,
}

struct Frame {
    step: usize,
    hops: usize,
    closed: bool,
    /// Entered by taking a hop: pop its path elements when done.
    pushed: bool,
    /// Entered by closing the previous step: pop that step's hop count when done.
    via_close: bool,
    list: Option<Vec<Hop>>,
    cursor: usize,
    bytes: usize,
}

pub(super) struct Walk<'a> {
    env: &'a SourceExecutionEnv<'a>,
    plan: &'a Plan<'a>,
    store: &'a Store,
    eval: &'a Evaluator<'a>,
    memory: &'a mut QueryMemoryReservation,
    mode: GraphPathMode,
    /// Result of each element-local inline predicate, by pattern slot and element.
    cache: HashMap<(usize, usize), bool>,
    vertices: Vec<usize>,
    edges: Vec<usize>,
    directions: Vec<u8>,
    step_hops: Vec<usize>,
    found: Vec<Candidate>,
    seen: HashSet<Vec<usize>>,
}

impl<'a> Walk<'a> {
    pub(super) fn new(
        env: &'a SourceExecutionEnv<'a>,
        plan: &'a Plan<'a>,
        store: &'a Store,
        eval: &'a Evaluator<'a>,
        memory: &'a mut QueryMemoryReservation,
    ) -> Self {
        Self {
            env,
            plan,
            store,
            eval,
            memory,
            mode: plan.table.effective_selector().mode,
            cache: HashMap::new(),
            vertices: Vec::new(),
            edges: Vec::new(),
            directions: Vec::new(),
            step_hops: Vec::new(),
            found: Vec::new(),
            seen: HashSet::new(),
        }
    }

    fn release(&mut self, bytes: usize) {
        let retained = self.memory.bytes();
        self.memory.shrink_to(retained.saturating_sub(bytes));
    }

    /// Every path matching labels, inline predicates, quantifier bounds and the mode, before
    /// any selector or graph-pattern `WHERE`.
    pub(super) fn run(mut self) -> Result<Vec<Candidate>, QueryError> {
        for start in 0..self.store.vertices.len() {
            self.env.controls.check_interruption()?;
            if !self.vertex_ok(0, start)? {
                continue;
            }
            self.vertices.clear();
            self.edges.clear();
            self.directions.clear();
            self.step_hops.clear();
            self.vertices.push(start);
            if self.plan.steps.is_empty() {
                self.emit(0)?;
            } else {
                self.search()?;
            }
        }
        Ok(self.found)
    }

    fn search(&mut self) -> Result<(), QueryError> {
        let mut stack = vec![self.frame(0, 0, false, false)?];
        while let Some(top) = stack.len().checked_sub(1) {
            self.env.controls.check_interruption()?;
            let (step, hops) = (stack[top].step, stack[top].hops);
            if !stack[top].closed {
                stack[top].closed = true;
                if self.close(step, hops)? {
                    stack.push(self.frame(step + 1, 0, false, true)?);
                    continue;
                }
            }
            if stack[top].list.is_none() {
                let list = self.hops_of(step, hops)?;
                let bytes = list.capacity() * size_of::<Hop>();
                self.memory.try_grow(bytes)?;
                stack[top].bytes += bytes;
                stack[top].list = Some(list);
            }
            let frame = &mut stack[top];
            let next = frame
                .list
                .as_ref()
                .and_then(|list| list.get(frame.cursor))
                .map(|hop| (hop.edge, hop.direction, hop.to));
            frame.cursor += 1;
            if let Some((edge, direction, to)) = next {
                if self.mode_allows(edge, to) {
                    self.vertices.push(to);
                    self.edges.push(edge);
                    self.directions.push(direction);
                    stack.push(self.frame(step, hops + 1, true, false)?);
                }
                continue;
            }
            let done = stack.pop().expect("frame on stack");
            if done.pushed {
                self.vertices.pop();
                self.edges.pop();
                self.directions.pop();
            }
            if done.via_close {
                self.step_hops.pop();
            }
            self.release(done.bytes);
        }
        Ok(())
    }

    fn frame(
        &mut self,
        step: usize,
        hops: usize,
        pushed: bool,
        via_close: bool,
    ) -> Result<Frame, QueryError> {
        let bytes = size_of::<Frame>() + 9;
        self.memory.try_grow(bytes)?;
        Ok(Frame {
            step,
            hops,
            closed: false,
            pushed,
            via_close,
            list: None,
            cursor: 0,
            bytes,
        })
    }

    /// Closes `step` after `hops` hops when its end vertex satisfies the declared vertex
    /// pattern. Returns whether the search continues into the next step.
    fn close(&mut self, step: usize, hops: usize) -> Result<bool, QueryError> {
        if hops < self.plan.steps[step].min {
            return Ok(false);
        }
        let last = *self.vertices.last().expect("path has a vertex");
        if !self.vertex_ok(step + 1, last)? || !self.repeats_vertex(step + 1, last) {
            return Ok(false);
        }
        if step + 1 == self.plan.steps.len() {
            self.emit(hops)?;
            return Ok(false);
        }
        self.step_hops.push(hops);
        Ok(true)
    }

    fn offset(&self, position: usize) -> usize {
        self.step_hops[..position].iter().copied().sum()
    }

    /// A repeated variable binds the same vertex at each of its positions.
    fn repeats_vertex(&self, position: usize, vertex: usize) -> bool {
        self.plan.positions[position]
            .repeat_of
            .is_none_or(|earlier| self.vertices[self.offset(earlier)] == vertex)
    }

    fn vertex_ok(&mut self, position: usize, vertex: usize) -> Result<bool, QueryError> {
        let place = &self.plan.positions[position];
        let (mapping, row) = self.store.vertex_row(vertex);
        let Some(columns) = &place.selection[mapping] else {
            return Ok(false);
        };
        let Some(filter) = place.filter.as_ref().filter(|filter| filter.local) else {
            return Ok(true);
        };
        let key = (position, vertex);
        if let Some(known) = self.cache.get(&key) {
            return Ok(*known);
        }
        let holds = self
            .eval
            .holds(filter.expr, element_entries(row, columns))?;
        self.remember(key, holds)?;
        Ok(holds)
    }

    fn edge_ok(&mut self, step: usize, edge: usize) -> Result<bool, QueryError> {
        let place = &self.plan.steps[step];
        let (mapping, row) = self.store.edge_row(edge);
        let Some(columns) = &place.selection[mapping] else {
            return Ok(false);
        };
        if let Some(earlier) = place.repeat_of {
            if self.edges[self.offset(earlier)] != edge {
                return Ok(false);
            }
        }
        let Some(filter) = place.filter.as_ref().filter(|filter| filter.local) else {
            return Ok(true);
        };
        let key = ((self.plan.positions.len() + step), edge);
        if let Some(known) = self.cache.get(&key) {
            return Ok(*known);
        }
        let holds = self
            .eval
            .holds(filter.expr, element_entries(row, columns))?;
        self.remember(key, holds)?;
        Ok(holds)
    }

    fn remember(&mut self, key: (usize, usize), holds: bool) -> Result<(), QueryError> {
        self.memory
            .try_grow(size_of::<((usize, usize), bool)>() + 2 * size_of::<usize>())?;
        self.cache.insert(key, holds);
        Ok(())
    }

    /// The traversal choices leaving the path's last vertex in `step`, when another hop is
    /// allowed. A self-loop is one step under an undirected edge, recorded as direction 1.
    fn hops_of(&mut self, step: usize, hops: usize) -> Result<Vec<Hop>, QueryError> {
        let place = &self.plan.steps[step];
        if place.max.is_some_and(|max| hops >= max) {
            return Ok(Vec::new());
        }
        let direction = place.direction;
        let from = *self.vertices.last().expect("path has a vertex");
        let store = self.store;
        let mut choices = Vec::new();
        if direction != GraphEdgeDirection::Incoming {
            for edge in &store.outgoing[from] {
                choices.push(Hop {
                    edge: *edge,
                    direction: 1,
                    to: store.edges[*edge].destination,
                });
            }
        }
        if direction != GraphEdgeDirection::Outgoing {
            for edge in &store.incoming[from] {
                let record = store.edges[*edge];
                if direction == GraphEdgeDirection::Undirected
                    && record.source == record.destination
                {
                    continue;
                }
                choices.push(Hop {
                    edge: *edge,
                    direction: 2,
                    to: record.source,
                });
            }
        }
        let mut allowed = Vec::with_capacity(choices.len());
        for choice in choices {
            self.env.controls.check_interruption()?;
            if self.edge_ok(step, choice.edge)? {
                allowed.push(choice);
            }
        }
        Ok(allowed)
    }

    /// Whether adding `edge` and the vertex it reaches keeps the path inside the mode.
    fn mode_allows(&self, edge: usize, to: usize) -> bool {
        match self.mode {
            GraphPathMode::Walk => true,
            GraphPathMode::Trail => !self.edges.contains(&edge),
            GraphPathMode::Acyclic => !self.vertices.contains(&to),
            GraphPathMode::Simple => {
                let first = self.vertices[0];
                let closed = self.vertices.len() > 1 && self.vertices.last() == Some(&first);
                !closed && !self.vertices[1..].contains(&to)
            }
        }
    }

    /// Records the path ending the last step after `hops` hops.
    fn emit(&mut self, hops: usize) -> Result<(), QueryError> {
        let mut step_hops = self.step_hops.clone();
        if !self.plan.steps.is_empty() {
            step_hops.push(hops);
        }
        let candidate = Candidate {
            vertices: self.vertices.clone(),
            edges: self.edges.clone(),
            directions: self.directions.clone(),
            step_hops,
        };
        if self.plan.deferred && !self.deferred_hold(&candidate)? {
            return Ok(());
        }
        if self.plan.may_duplicate {
            // Hop splits of one path are one match unless they bind a named singleton
            // variable to different elements; then each binding is its own match.
            let mut key = candidate.vertices.clone();
            key.extend(&candidate.edges);
            key.extend(candidate.directions.iter().map(|d| usize::from(*d)));
            key.extend(
                self.plan
                    .variables
                    .iter()
                    .filter_map(|variable| match variable.source {
                        Source::Vertex(position) | Source::Edge(position) => {
                            Some(candidate.offset(position))
                        }
                        Source::Group => None,
                    }),
            );
            self.memory
                .try_grow(key.capacity() * 4 + 2 * size_of::<usize>())?;
            if !self.seen.insert(key) {
                return Ok(());
            }
        }
        self.memory.try_grow(candidate.bytes())?;
        self.found.push(candidate);
        Ok(())
    }

    /// Inline predicates that read other pattern variables run on the complete path, still
    /// before any selector.
    fn deferred_hold(&mut self, candidate: &Candidate) -> Result<bool, QueryError> {
        let plan = self.plan;
        let scalars = scalar_entries(plan, self.store, candidate);
        for place in &plan.positions {
            let Some(filter) = place.filter.as_ref().filter(|filter| !filter.local) else {
                continue;
            };
            if !self.eval.holds(filter.expr, scalars.clone())? {
                return Ok(false);
            }
        }
        for (index, step) in plan.steps.iter().enumerate() {
            let Some(filter) = step.filter.as_ref().filter(|filter| !filter.local) else {
                continue;
            };
            if !step.group {
                if !self.eval.holds(filter.expr, scalars.clone())? {
                    return Ok(false);
                }
                continue;
            }
            let first = candidate.offset(index);
            let last = first + candidate.step_hops[index];
            for edge in &candidate.edges[first..last] {
                self.env.controls.check_interruption()?;
                let mut entries = scalars.clone();
                entries.extend(group_entries(plan, self.store, index, *edge));
                if !self.eval.holds(filter.expr, entries)? {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
}
