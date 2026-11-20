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
use super::cache::{reserve_map, reserve_set};
use super::eval::{combine_entries, element_entries, Evaluator};
use super::plan::{Plan, Source};
use super::store::Store;
use super::{QueryError, SourceExecutionEnv};
use crate::executor::retained_memory::{add, mul};
use crate::runtime::owned_memory::{allocation_error, grow_vec};
use crate::runtime::{QueryMemoryReservation, QueryScoringTestBoundary};
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
    store: &'a mut Store,
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
    /// Start vertices fixed by an earlier pattern of the table (#1353), replacing the
    /// pattern's own start lookup.
    starts: Option<Vec<usize>>,
    /// Edge hops the search took: neighborhood edges that passed the step's mapping and
    /// inline predicate.
    expansions: u64,
}

impl<'a> Walk<'a> {
    pub(super) fn new(
        env: &'a SourceExecutionEnv<'a>,
        plan: &'a Plan<'a>,
        store: &'a mut Store,
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
            starts: None,
            expansions: 0,
        }
    }

    /// Starts the search only at `starts`, in order, instead of looking them up.
    pub(super) fn with_starts(mut self, starts: Vec<usize>) -> Self {
        self.starts = Some(starts);
        self
    }

    fn release(&mut self, bytes: usize) {
        let retained = self.memory.bytes();
        self.memory.shrink_to(retained.saturating_sub(bytes));
    }

    /// Every path matching labels, inline predicates, quantifier bounds and the mode, before
    /// any selector or graph-pattern `WHERE`.
    ///
    /// Returns the paths and the number of edge hops the search took.
    pub(super) fn run(mut self) -> Result<(Vec<Candidate>, u64), QueryError> {
        let starts = match self.starts.take() {
            Some(starts) => starts,
            None => self
                .store
                .start_vertices(self.env, self.plan, self.eval, self.memory)?,
        };
        for start in starts {
            self.env.controls.check_interruption()?;
            if !self.vertex_ok(0, start)? {
                continue;
            }
            self.vertices.clear();
            self.edges.clear();
            self.directions.clear();
            self.step_hops.clear();
            grow_vec(&mut self.vertices, 1, self.memory)?;
            self.vertices.push(start);
            if self.plan.steps.is_empty() {
                self.emit(0)?;
            } else {
                self.search()?;
            }
        }
        Ok((self.found, self.expansions))
    }

    fn search(&mut self) -> Result<(), QueryError> {
        let mut stack_memory = self.env.controls.reserve_query_memory(0)?;
        let mut stack = Vec::new();
        grow_vec(&mut stack, 1, &mut stack_memory)?;
        stack.push(Self::frame(0, 0, false, false));
        while let Some(top) = stack.len().checked_sub(1) {
            self.env
                .controls
                .scored_candidate(QueryScoringTestBoundary::GraphTraversal)?;
            self.env.controls.check_interruption()?;
            let (step, hops) = (stack[top].step, stack[top].hops);
            if !stack[top].closed {
                stack[top].closed = true;
                if self.close(step, hops)? {
                    grow_vec(&mut stack, 1, &mut stack_memory)?;
                    stack.push(Self::frame(step + 1, 0, false, true));
                    continue;
                }
            }
            if stack[top].list.is_none() {
                let list = self.hops_of(step, hops)?;
                let bytes = list.capacity() * size_of::<Hop>();
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
                    grow_vec(&mut self.vertices, 1, self.memory)?;
                    grow_vec(&mut self.edges, 1, self.memory)?;
                    grow_vec(&mut self.directions, 1, self.memory)?;
                    self.vertices.push(to);
                    self.edges.push(edge);
                    self.directions.push(direction);
                    grow_vec(&mut stack, 1, &mut stack_memory)?;
                    stack.push(Self::frame(step, hops + 1, true, false));
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
            let bytes = done.bytes;
            drop(done);
            self.release(bytes);
        }
        Ok(())
    }

    fn frame(step: usize, hops: usize, pushed: bool, via_close: bool) -> Frame {
        Frame {
            step,
            hops,
            closed: false,
            pushed,
            via_close,
            list: None,
            cursor: 0,
            bytes: 0,
        }
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
        grow_vec(&mut self.step_hops, 1, self.memory)?;
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
        let entries = element_entries(self.env, row, columns)?;
        let holds = self.eval.holds(filter.expr, entries.get())?;
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
        let entries = element_entries(self.env, row, columns)?;
        let holds = self.eval.holds(filter.expr, entries.get())?;
        self.remember(key, holds)?;
        Ok(holds)
    }

    fn remember(&mut self, key: (usize, usize), holds: bool) -> Result<(), QueryError> {
        reserve_map(&mut self.cache, self.memory)?;
        self.cache.insert(key, holds);
        Ok(())
    }

    /// The traversal choices leaving the path's last vertex in `step`, when another hop is
    /// allowed. A self-loop is one step under an undirected edge, recorded as direction 1.
    fn hops_of(&mut self, step: usize, hops: usize) -> Result<Vec<Hop>, QueryError> {
        let plan = self.plan;
        let place = &plan.steps[step];
        if place.max.is_some_and(|max| hops >= max) {
            return Ok(Vec::new());
        }
        let direction = place.direction;
        let from = *self.vertices.last().expect("path has a vertex");
        self.store
            .expand(self.env, &plan.graph, from, (direction, &place.selection))?;
        let store = &*self.store;
        #[cfg(test)]
        crate::executor::execution::graph_memory_test_observer::record(
            "before_frontier",
            self.memory.bytes(),
        );
        let capacity = add(
            if direction == GraphEdgeDirection::Incoming {
                0
            } else {
                store.outgoing[from].len()
            },
            if direction == GraphEdgeDirection::Outgoing {
                0
            } else {
                store.incoming[from].len()
            },
        )?;
        let bytes = mul(capacity, size_of::<Hop>())?;
        self.memory.try_grow(mul(bytes, 2)?)?;
        #[cfg(test)]
        crate::executor::execution::graph_memory_test_observer::record(
            "copy_frontier",
            self.memory.bytes(),
        );
        let mut choices = Vec::new();
        choices
            .try_reserve_exact(capacity)
            .map_err(|error| allocation_error(&error))?;
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
        let mut allowed = Vec::new();
        allowed
            .try_reserve_exact(capacity)
            .map_err(|error| allocation_error(&error))?;
        if choices.capacity() > capacity || allowed.capacity() > capacity {
            return Err(crate::app::CassieError::ResourceLimit(
                "allocator exceeded admitted graph frontier capacity".to_owned(),
            )
            .into());
        }
        for choice in choices {
            self.env.controls.check_interruption()?;
            if self.edge_ok(step, choice.edge)? {
                allowed.push(choice);
            }
        }
        self.expansions = self.expansions.saturating_add(allowed.len() as u64);
        self.release(bytes);
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
        #[cfg(test)]
        crate::executor::execution::graph_memory_test_observer::record(
            "before_candidate",
            self.memory.bytes(),
        );
        let (step_count, candidate_bytes) = self.candidate_shape()?;
        self.memory.try_grow(candidate_bytes)?;
        #[cfg(test)]
        crate::executor::execution::graph_memory_test_observer::record(
            "copy_candidate",
            self.memory.bytes(),
        );
        let mut step_hops = Vec::with_capacity(step_count);
        step_hops.extend_from_slice(&self.step_hops);
        if !self.plan.steps.is_empty() {
            step_hops.push(hops);
        }
        let candidate = Candidate {
            vertices: self.vertices.clone(),
            edges: self.edges.clone(),
            directions: self.directions.clone(),
            step_hops,
        };
        if candidate.bytes() > candidate_bytes {
            return Err(crate::app::CassieError::ResourceLimit(
                "candidate exceeded admitted graph path capacity".to_owned(),
            )
            .into());
        }
        if self.plan.deferred && !self.deferred_hold(&candidate)? {
            drop(candidate);
            self.release(candidate_bytes);
            return Ok(());
        }
        if self.plan.may_duplicate {
            // Hop splits of one path are one match unless they bind a named singleton
            // variable to different elements; then each binding is its own match.
            #[cfg(test)]
            crate::executor::execution::graph_memory_test_observer::record(
                "before_seen_key",
                self.memory.bytes(),
            );
            let singletons = self
                .plan
                .variables
                .iter()
                .filter(|variable| !matches!(variable.source, Source::Group))
                .count();
            let length = add(
                add(
                    add(candidate.vertices.len(), candidate.edges.len())?,
                    candidate.directions.len(),
                )?,
                singletons,
            )?;
            let bytes = add(mul(length, size_of::<usize>())?, 2 * size_of::<usize>())?;
            self.memory.try_grow(bytes)?;
            #[cfg(test)]
            crate::executor::execution::graph_memory_test_observer::record(
                "copy_seen_key",
                self.memory.bytes(),
            );
            let mut key = Vec::with_capacity(length);
            key.extend_from_slice(&candidate.vertices);
            key.extend(&candidate.edges);
            key.extend(candidate.directions.iter().map(|d| usize::from(*d)));
            key.extend(
                self.plan
                    .variables
                    .iter()
                    .filter_map(|variable| match variable.source {
                        Source::Vertex(position) => {
                            Some(candidate.vertices[candidate.offset(position)])
                        }
                        Source::Edge(position) => Some(candidate.edges[candidate.offset(position)]),
                        Source::Group => None,
                    }),
            );
            #[cfg(test)]
            crate::executor::execution::graph_memory_test_observer::record(
                "required_seen_key",
                key.capacity() * size_of::<usize>() + 2 * size_of::<usize>(),
            );
            #[cfg(test)]
            crate::executor::execution::graph_memory_test_observer::record(
                "charged_seen_key",
                bytes,
            );
            if self.seen.contains(&key) {
                drop(key);
                drop(candidate);
                self.release(add(bytes, candidate_bytes)?);
                return Ok(());
            }
            reserve_set(&mut self.seen, self.memory)?;
            self.seen.insert(key);
        }
        grow_vec(&mut self.found, 1, self.memory)?;
        self.found.push(candidate);
        Ok(())
    }

    /// The populated step slots and admitted bytes before a candidate is copied.
    fn candidate_shape(&self) -> Result<(usize, usize), QueryError> {
        let step_count = add(
            self.step_hops.len(),
            usize::from(!self.plan.steps.is_empty()),
        )?;
        let index_count = add(add(self.vertices.len(), self.edges.len())?, step_count)?;
        let candidate_bytes = add(
            size_of::<Candidate>(),
            add(mul(index_count, size_of::<usize>())?, self.directions.len())?,
        )?;
        Ok((step_count, candidate_bytes))
    }

    /// Inline predicates that read other pattern variables run on the complete path, still
    /// before any selector.
    fn deferred_hold(&mut self, candidate: &Candidate) -> Result<bool, QueryError> {
        let plan = self.plan;
        let scalars = scalar_entries(self.env, plan, &*self.store, candidate)?;
        for place in &plan.positions {
            let Some(filter) = place.filter.as_ref().filter(|filter| !filter.local) else {
                continue;
            };
            if !self.eval.holds(filter.expr, scalars.get())? {
                return Ok(false);
            }
        }
        for (index, step) in plan.steps.iter().enumerate() {
            let Some(filter) = step.filter.as_ref().filter(|filter| !filter.local) else {
                continue;
            };
            if !step.group {
                if !self.eval.holds(filter.expr, scalars.get())? {
                    return Ok(false);
                }
                continue;
            }
            let first = candidate.offset(index);
            let last = first + candidate.step_hops[index];
            for edge in &candidate.edges[first..last] {
                self.env.controls.check_interruption()?;
                let group = group_entries(self.env, plan, &*self.store, index, *edge)?;
                let entries = combine_entries(self.env, scalars.get(), group.get())?;
                if !self.eval.holds(filter.expr, entries.get())? {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod memory_tests {
    use crate::executor::execution::graph_memory_test_observer as observer;

    #[test]
    fn should_admit_a_candidate_before_copying_its_path_vectors() {
        // Arrange
        let (cassie, session) = observer::fixture();
        let _observation = observer::start();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT * FROM GRAPH_TABLE(g MATCH(a IS node WHERE a.id=1)-[IS edge]->(b IS node) \
             COLUMNS(b.id)) AS t",
                vec![],
            )
            .expect("one graph path");

        // Assert
        assert_eq!(result.row_count(), 1);
        observer::assert_admitted(
            "before_candidate",
            "copy_candidate",
            std::mem::size_of::<super::Candidate>() + 4 * std::mem::size_of::<usize>() + 1,
        );
    }

    #[test]
    fn should_admit_the_traversal_frontier_before_allocating_hop_choices() {
        // Arrange
        let (cassie, session) = observer::fixture();
        let _observation = observer::start();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT * FROM GRAPH_TABLE(g MATCH(a IS node WHERE a.id=1)-[IS edge]->(b IS node) \
             COLUMNS(b.id)) AS t",
                vec![],
            )
            .expect("one graph path");

        // Assert
        assert_eq!(result.row_count(), 1);
        observer::assert_admitted(
            "before_frontier",
            "copy_frontier",
            std::mem::size_of::<super::Hop>(),
        );
    }

    fn duplicate_fixture() -> (crate::app::Cassie, crate::app::CassieSession) {
        let (cassie, session) = observer::fixture();
        for sql in ["DELETE FROM e", "INSERT INTO e VALUES(1,1,1)"] {
            cassie
                .execute_sql(&session, sql, vec![])
                .expect("one self loop");
        }
        (cassie, session)
    }

    const DUPLICATE_QUERY: &str = "SELECT * FROM GRAPH_TABLE(g MATCH \
        (a IS node WHERE a.id=1)-[]->{0,1}(b)-[]->{0,1}(c) COLUMNS(a.id)) AS t";

    #[test]
    fn should_admit_a_seen_key_before_copying_its_indices() {
        // Arrange
        let (cassie, session) = duplicate_fixture();
        let _observation = observer::start();

        // Act
        let result = cassie
            .execute_sql(&session, DUPLICATE_QUERY, vec![])
            .expect("quantified self loop");

        // Assert
        assert!(result.row_count() > 0);
        observer::assert_admitted(
            "before_seen_key",
            "copy_seen_key",
            std::mem::size_of::<usize>(),
        );
    }

    #[test]
    fn should_account_for_the_native_width_of_retained_seen_key_indices() {
        // Arrange
        let (cassie, session) = duplicate_fixture();
        let _observation = observer::start();

        // Act
        let result = cassie
            .execute_sql(&session, DUPLICATE_QUERY, vec![])
            .expect("quantified self loop");
        let observations = observer::events();

        // Assert
        assert!(result.row_count() > 0);
        let mut required = None;
        let mut checked = 0;
        for (boundary, bytes) in observations {
            if boundary == "required_seen_key" {
                required = Some(bytes);
            } else if boundary == "charged_seen_key" {
                let required = required
                    .take()
                    .expect("native capacity oracle before admission");
                assert!(
                    bytes >= required,
                    "seen key admitted {bytes} bytes but owns at least {required}"
                );
                checked += 1;
            }
        }
        assert!(checked > 0, "the fixture must retain a seen key");
    }
}
