//! Comma-separated path patterns joined on shared vertex variables (#1353).
//!
//! Every pattern is searched and selected on its own over the one statement store, each
//! under its own mode and selector. The selected paths are then joined on the vertex
//! identities of the variables the patterns share: a vertex identity is its mapping and key
//! tuple, so equal printed key components under different mappings (tenants) never match.
//! Branch `i` starts only at the vertices an earlier branch bound when its first vertex is a
//! shared variable. The output order is lexicographic over the position of each branch's path in
//! that branch's documented path order.

use std::collections::HashMap;
use std::mem::size_of;

use super::bindings::{scalar_entries, Candidate};
use super::cache::reserve_map;
use super::eval::{push_entry, Entries, Evaluator};
use super::plan::{Plan, Source};
use super::select;
use super::store::Store;
use super::walk::Walk;
use super::{QueryError, SourceExecutionEnv};
use crate::runtime::accounted::{Accounted, AccountedVec};
use crate::runtime::owned_memory::grow_vec;
use crate::runtime::QueryMemoryReservation;

/// The matches of a table: the selected paths of its only pattern, or the join of the
/// selected paths of several.
pub(super) enum Matches {
    One(Vec<Candidate>),
    Joined(Joined),
}

/// Join tuples: `flat` holds `branches.len()` indexes per match, one into each branch's
/// selected paths.
pub(super) struct Joined {
    branches: Vec<Vec<Candidate>>,
    flat: Vec<usize>,
}

/// What one branch of a joined table did, for the runtime metrics.
#[derive(Clone, Copy, Default)]
pub(super) struct BranchStats {
    /// Edge hops the branch's search took.
    pub(super) expansions: u64,
    /// Paths the branch selected, before the join.
    pub(super) paths: u64,
}

impl Matches {
    pub(super) fn len(&self) -> usize {
        match self {
            Self::One(paths) => paths.len(),
            Self::Joined(joined) => joined.flat.len() / joined.branches.len().max(1),
        }
    }

    /// The path of match `index` when the table has one pattern (the only case that can
    /// project a path variable).
    pub(super) fn single(&self, index: usize) -> Option<&Candidate> {
        match self {
            Self::One(paths) => paths.get(index),
            Self::Joined(_) => None,
        }
    }

    /// `variable.property` entries of every scalar variable of match `index`. A shared
    /// variable contributes each property once.
    pub(super) fn entries(
        &self,
        env: &SourceExecutionEnv<'_>,
        plans: &[Plan<'_>],
        store: &Store,
        index: usize,
    ) -> Result<Accounted<Entries>, QueryError> {
        match self {
            Self::One(paths) => scalar_entries(env, &plans[0], store, &paths[index]),
            Self::Joined(joined) => {
                let width = joined.branches.len();
                let mut entries = AccountedVec::try_new(env.controls)?;
                let mut names = Vec::<String>::new();
                for (branch, plan) in plans.iter().enumerate() {
                    let path = &joined.branches[branch][joined.flat[index * width + branch]];
                    let part = scalar_entries(env, plan, store, path)?;
                    for (name, value) in part.get() {
                        if names.iter().any(|known| known == name) {
                            continue;
                        }
                        env.controls.check_interruption()?;
                        push_entry(&mut entries, name, Some(value))?;
                        names.push(name.clone());
                    }
                }
                let (entries, memory) = entries.into_parts();
                Ok(Accounted::from_admitted(entries, memory))
            }
        }
    }
}

/// The vertex identity `plan`'s variable `name` binds in `path`, when it is a vertex.
fn vertex_of(plan: &Plan<'_>, name: &str, path: &Candidate) -> Option<usize> {
    plan.variables
        .iter()
        .find(|variable| variable.name == name)
        .and_then(|variable| match variable.source {
            Source::Vertex(position) => Some(path.vertices[path.offset(position)]),
            _ => None,
        })
}

/// Vertex variables of `plan` that an earlier plan also binds, as `(name, earlier plan)`.
fn shared_with_earlier(plans: &[Plan<'_>], branch: usize) -> Vec<(String, usize)> {
    plans[branch]
        .variables
        .iter()
        .filter(|variable| matches!(variable.source, Source::Vertex(_)))
        .filter_map(|variable| {
            plans[..branch]
                .iter()
                .position(|earlier| {
                    earlier.variables.iter().any(|known| {
                        known.name == variable.name && matches!(known.source, Source::Vertex(_))
                    })
                })
                .map(|earlier| (variable.name.clone(), earlier))
        })
        .collect()
}

/// The vertices an earlier branch bound to the variable that starts `branch`, sorted and
/// distinct; `None` when the start is not a shared variable.
fn restricted_starts(
    plans: &[Plan<'_>],
    selected: &[Vec<Candidate>],
    branch: usize,
    memory: &mut QueryMemoryReservation,
) -> Result<Option<Vec<usize>>, QueryError> {
    let Some(start) = plans[branch].positions[0].variable else {
        return Ok(None);
    };
    let name = &plans[branch].variables[start].name;
    let Some(earlier) = shared_with_earlier(plans, branch)
        .into_iter()
        .find_map(|(shared, earlier)| (shared == *name).then_some(earlier))
    else {
        return Ok(None);
    };
    let mut starts = Vec::new();
    for path in &selected[earlier] {
        grow_vec(&mut starts, 1, memory)?;
        starts.push(vertex_of(&plans[earlier], name, path).expect("shared vertex variable"));
    }
    starts.sort_unstable();
    starts.dedup();
    Ok(Some(starts))
}

/// Searches, selects and joins every pattern of the table.
///
/// # Errors
///
/// Cancellation and the query memory budget, which fail the statement rather than return a
/// partial match set.
pub(super) fn run(
    env: &SourceExecutionEnv<'_>,
    plans: &[Plan<'_>],
    store: &mut Store,
    eval: &Evaluator<'_>,
    memory: &mut QueryMemoryReservation,
) -> Result<(Matches, Vec<BranchStats>), QueryError> {
    let kind = plans[0].table.effective_selector().kind;
    let mut selected = Vec::with_capacity(plans.len());
    let mut stats = Vec::with_capacity(plans.len());
    for branch in 0..plans.len() {
        env.controls.check_interruption()?;
        let fixed = restricted_starts(plans, &selected, branch, memory)?;
        let mut walk = Walk::new(env, &plans[branch], store, eval, memory);
        if let Some(fixed) = fixed {
            walk = walk.with_starts(fixed);
        }
        let (found, expansions) = walk.run()?;
        let paths = select::apply(env, store, kind, found)?;
        stats.push(BranchStats {
            expansions,
            paths: paths.len() as u64,
        });
        selected.push(paths);
    }
    if plans.len() == 1 {
        let only = selected.pop().expect("one branch");
        return Ok((Matches::One(only), stats));
    }
    let flat = join(env, plans, &selected, memory)?;
    Ok((
        Matches::Joined(Joined {
            branches: selected,
            flat,
        }),
        stats,
    ))
}

/// Joins the branches in order; each tuple extends the earlier ones, so the result is
/// already in lexicographic order of branch path positions.
fn join(
    env: &SourceExecutionEnv<'_>,
    plans: &[Plan<'_>],
    selected: &[Vec<Candidate>],
    memory: &mut QueryMemoryReservation,
) -> Result<Vec<usize>, QueryError> {
    let mut flat = Vec::<usize>::new();
    grow_vec(&mut flat, selected[0].len(), memory)?;
    flat.extend(0..selected[0].len());
    for branch in 1..plans.len() {
        let shared = shared_with_earlier(plans, branch);
        let keyed = index_branch(env, &plans[branch], &selected[branch], &shared, memory)?;
        let width = branch;
        let mut next = Vec::<usize>::new();
        for tuple in flat.chunks_exact(width) {
            env.controls
                .scored_candidate(crate::runtime::QueryScoringTestBoundary::GraphJoin)?;
            env.controls.check_interruption()?;
            let key = shared
                .iter()
                .map(|(name, earlier)| {
                    vertex_of(&plans[*earlier], name, &selected[*earlier][tuple[*earlier]])
                        .expect("shared vertex variable")
                })
                .collect::<Vec<_>>();
            let Some(matching) = keyed.get(&key) else {
                continue;
            };
            for candidate in matching {
                env.controls.check_interruption()?;
                grow_vec(&mut next, width + 1, memory)?;
                next.extend_from_slice(tuple);
                next.push(*candidate);
            }
        }
        let released = flat.capacity().saturating_mul(size_of::<usize>());
        drop(std::mem::replace(&mut flat, next));
        memory.shrink_to(memory.bytes().saturating_sub(released));
    }
    Ok(flat)
}

/// The branch's paths grouped by the identities of the shared vertex variables, each group
/// in path order.
fn index_branch(
    env: &SourceExecutionEnv<'_>,
    plan: &Plan<'_>,
    paths: &[Candidate],
    shared: &[(String, usize)],
    memory: &mut QueryMemoryReservation,
) -> Result<HashMap<Vec<usize>, Vec<usize>>, QueryError> {
    let mut keyed = HashMap::<Vec<usize>, Vec<usize>>::new();
    for (index, path) in paths.iter().enumerate() {
        env.controls.check_interruption()?;
        let key = shared
            .iter()
            .map(|(name, _)| vertex_of(plan, name, path).expect("shared vertex variable"))
            .collect::<Vec<_>>();
        if !keyed.contains_key(&key) {
            reserve_map(&mut keyed, memory)?;
            memory.try_grow(key.len() * size_of::<usize>())?;
        }
        let group = keyed.entry(key).or_default();
        grow_vec(group, 1, memory)?;
        group.push(index);
    }
    Ok(keyed)
}
