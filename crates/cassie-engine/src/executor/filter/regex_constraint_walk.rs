//! Input-owned constraint evaluation with admitted iterative repetition frontiers.

use std::cmp::Ordering;

use super::{
    check_interruption, memory::Directory, resource_limit, Kind, LookAround, Node, QueryError,
    Regex,
};
use crate::runtime::QueryExecutionControls;
use regex_automata::{Anchored, Input, Span};

const MAX_STEPS: usize = 10_000_000;

#[derive(Clone, Copy)]
struct Tag {
    node: usize,
    span: Span,
}

pub(super) struct State {
    pub(super) position: usize,
    pub(super) captures: Directory<Option<Span>>,
    tags: Directory<Tag>,
}

impl State {
    fn new(
        position: usize,
        groups: usize,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<Self, QueryError> {
        let mut captures = Directory::with_capacity(groups, controls)?;
        captures.values.resize(groups, None);
        Ok(Self {
            position,
            captures,
            tags: Directory::new(),
        })
    }

    fn copy(&self, controls: Option<&QueryExecutionControls>) -> Result<Self, QueryError> {
        Ok(Self {
            position: self.position,
            captures: self.captures.copy(controls)?,
            tags: self.tags.copy(controls)?,
        })
    }

    fn clear(&mut self, node: &Node) {
        self.captures[node.captures.clone()].fill(None);
    }
}

impl Regex {
    pub(super) fn search(
        &self,
        input: &Input<'_>,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<Option<State>, QueryError> {
        let mut check = || check_interruption(controls);
        search(self, input, controls, &mut check)
    }
}

fn search(
    regex: &Regex,
    input: &Input<'_>,
    controls: Option<&QueryExecutionControls>,
    check: &mut impl FnMut() -> Result<(), QueryError>,
) -> Result<Option<State>, QueryError> {
    let text = std::str::from_utf8(input.haystack())
        .map_err(|_| resource_limit("constraint input is not UTF-8"))?;
    let mut walk = Walk {
        regex,
        text,
        controls,
        remaining: MAX_STEPS,
        check,
    };
    let mut start = input.start();
    loop {
        walk.tick()?;
        let state = State::new(start, regex.groups, controls)?;
        let mut results = walk.eval(0, state, input.end())?;
        let mut selected: Option<State> = None;
        for mut result in results.values.drain(..) {
            walk.tick()?;
            result.captures[0] = Some(Span::from(start..result.position));
            if selected
                .as_ref()
                .map_or(Ok(true), |previous| walk.better(&result, previous))?
            {
                selected = Some(result);
            }
        }
        if selected.is_some() {
            return Ok(selected);
        }
        if input.get_anchored() != Anchored::No || start == input.end() {
            return Ok(None);
        }
        let Some(character) = text[start..input.end()].chars().next() else {
            return Ok(None);
        };
        start += character.len_utf8();
    }
}

struct Walk<'a, 'b, F> {
    regex: &'a Regex,
    text: &'a str,
    controls: Option<&'a QueryExecutionControls>,
    remaining: usize,
    check: &'b mut F,
}

impl<F: FnMut() -> Result<(), QueryError>> Walk<'_, '_, F> {
    fn tick(&mut self) -> Result<(), QueryError> {
        (self.check)()?;
        self.remaining = self
            .remaining
            .checked_sub(1)
            .ok_or_else(|| resource_limit("constraint execution step limit exceeded"))?;
        Ok(())
    }

    fn single(&self, state: State) -> Result<Directory<State>, QueryError> {
        let mut states = Directory::with_capacity(1, self.controls)?;
        states.push(state, self.controls)?;
        Ok(states)
    }

    fn eval(
        &mut self,
        id: usize,
        mut state: State,
        end: usize,
    ) -> Result<Directory<State>, QueryError> {
        self.tick()?;
        let regex = self.regex;
        let node = &regex.nodes[id];
        let start = state.position;
        let tag = state.tags.len();
        state.tags.push(
            Tag {
                node: id,
                span: Span::from(start..start),
            },
            self.controls,
        )?;
        let mut results = match &node.kind {
            Kind::Empty => self.single(state)?,
            Kind::Atom(atom) => {
                let matched = atom[0].find(
                    Input::new(self.text)
                        .range(start..self.text.len())
                        .anchored(Anchored::Yes),
                );
                self.tick()?;
                if let Some(matched) = matched.filter(|matched| matched.end() <= end) {
                    state.position = matched.end();
                    self.single(state)?
                } else {
                    Directory::new()
                }
            }
            Kind::Concat(children) => self.concat(children, state, end)?,
            Kind::Alt(children) => {
                let mut states = Directory::new();
                for child in children.iter() {
                    let mut branch = self.eval(*child, state.copy(self.controls)?, end)?;
                    states.append(&mut branch, self.controls)?;
                }
                states
            }
            Kind::Group { child, slot } => {
                state.clear(node);
                let mut states = self.eval(*child, state, end)?;
                for result in states.iter_mut() {
                    self.tick()?;
                    result.captures[*slot] = Some(Span::from(start..result.position));
                }
                states
            }
            Kind::Repeat {
                child,
                lo,
                hi,
                split_final,
            } => {
                state.clear(node);
                self.repeat(id, *child, *lo..=*hi, *split_final, state, end)?
            }
            Kind::Look { child, kind } => {
                if self.look(*child, *kind, start)? {
                    self.single(state)?
                } else {
                    Directory::new()
                }
            }
            Kind::Backref { slot, casei } => {
                if let Some(position) = self.backref(&state, *slot, *casei, end)? {
                    state.position = position;
                    self.single(state)?
                } else {
                    Directory::new()
                }
            }
        };
        for result in results.iter_mut() {
            self.tick()?;
            result.tags[tag].span.end = result.position;
        }
        Ok(results)
    }

    fn concat(
        &mut self,
        children: &[usize],
        state: State,
        end: usize,
    ) -> Result<Directory<State>, QueryError> {
        let mut states = self.single(state)?;
        for child in children {
            self.tick()?;
            let mut next = Directory::new();
            for state in states.values.drain(..) {
                let mut results = self.eval(*child, state, end)?;
                next.append(&mut results, self.controls)?;
            }
            states = next;
            if states.is_empty() {
                break;
            }
        }
        Ok(states)
    }

    fn repeat(
        &mut self,
        id: usize,
        child: usize,
        bounds: std::ops::RangeInclusive<usize>,
        split_final: bool,
        state: State,
        end: usize,
    ) -> Result<Directory<State>, QueryError> {
        let (lo, hi) = (*bounds.start(), *bounds.end());
        let start = state.position;
        let tag_floor = state.tags.len();
        let mut accepted = Directory::new();
        if lo == 0 {
            accepted.push(state.copy(self.controls)?, self.controls)?;
        }
        let mut frontier = self.single(state)?;
        let mut count = 0_usize;
        while count < hi && !frontier.is_empty() {
            self.tick()?;
            count += 1;
            let mut next = Directory::new();
            for mut state in frontier.values.drain(..) {
                let before = state.position;
                state.clear(&self.regex.nodes[child]);
                if split_final {
                    self.clear_tags(&mut state, child, tag_floor)?;
                }
                let mut results = self.eval(child, state, end)?;
                for result in results.values.drain(..) {
                    self.tick()?;
                    let empty = result.position == before;
                    // An unnecessary trailing empty iteration cannot erase the
                    // last consumed capture. Required nullable iterations remain.
                    if empty && !split_final && count > lo && count > 1 {
                        continue;
                    }
                    if count >= lo {
                        let mut result = result.copy(self.controls)?;
                        if split_final {
                            // PostgreSQL regcomp's backref-free positive-minimum
                            // rewrite prioritizes x{m-1,n-1} before the final x.
                            result.tags.push(
                                Tag {
                                    node: id,
                                    span: Span::from(start..before),
                                },
                                self.controls,
                            )?;
                        }
                        accepted.push(result, self.controls)?;
                    }
                    if !empty || count < lo {
                        next.push(result, self.controls)?;
                    }
                }
            }
            frontier = next;
        }
        Ok(accepted)
    }

    fn clear_tags(
        &mut self,
        state: &mut State,
        child: usize,
        floor: usize,
    ) -> Result<(), QueryError> {
        let end = self.regex.nodes[child].subtree_end;
        let mut retained = 0;
        for index in 0..state.tags.len() {
            self.tick()?;
            let tag = state.tags[index];
            if index < floor || !(child..end).contains(&tag.node) {
                state.tags[retained] = tag;
                retained += 1;
            }
        }
        state.tags.values.truncate(retained);
        Ok(())
    }

    fn look(
        &mut self,
        child: usize,
        kind: LookAround,
        position: usize,
    ) -> Result<bool, QueryError> {
        let behind = matches!(kind, LookAround::LookBehind | LookAround::LookBehindNeg);
        let negative = matches!(kind, LookAround::LookAheadNeg | LookAround::LookBehindNeg);
        let mut start = if behind { 0 } else { position };
        loop {
            self.tick()?;
            let state = State::new(start, self.regex.groups, self.controls)?;
            let results = self.eval(
                child,
                state,
                if behind { position } else { self.text.len() },
            )?;
            for state in results.iter() {
                self.tick()?;
                if !behind || state.position == position {
                    return Ok(!negative);
                }
            }
            if !behind || start == position {
                return Ok(negative);
            }
            let character = self.text[start..position]
                .chars()
                .next()
                .expect("lookbehind scalar boundary");
            start += character.len_utf8();
        }
    }

    fn backref(
        &mut self,
        state: &State,
        slot: usize,
        casei: bool,
        end: usize,
    ) -> Result<Option<usize>, QueryError> {
        let Some(span) = state.captures[slot] else {
            return Ok(None);
        };
        let captured = &self.text[span.start..span.end];
        if casei {
            let escaped = super::escape(captured, self.controls)?;
            let atom = super::bounded::Regex::build(
                escaped.as_str(),
                regex_automata::util::syntax::Config::new().case_insensitive(true),
                None,
                self.controls,
            )?;
            let matched = atom.find(
                Input::new(self.text)
                    .range(state.position..self.text.len())
                    .anchored(Anchored::Yes),
            );
            self.tick()?;
            return Ok(matched
                .filter(|matched| matched.end() <= end)
                .map(|matched| matched.end()));
        }
        let Some(after) = state
            .position
            .checked_add(captured.len())
            .filter(|after| *after <= end)
        else {
            return Ok(None);
        };
        let candidate = &self.text.as_bytes()[state.position..after];
        for (expected, actual) in captured.as_bytes().chunks(1024).zip(candidate.chunks(1024)) {
            self.tick()?;
            if expected != actual {
                return Ok(None);
            }
        }
        Ok(Some(after))
    }

    fn better(&mut self, candidate: &State, previous: &State) -> Result<bool, QueryError> {
        match candidate.position.cmp(&previous.position) {
            Ordering::Greater => return Ok(true),
            Ordering::Less => return Ok(false),
            Ordering::Equal => {}
        }
        // A repeated node's first occurrence has priority over its later
        // occurrences; its public capture still exposes the final iteration.
        for node in 0..self.regex.nodes.len() {
            let mut left = 0;
            let mut right = 0;
            loop {
                self.tick()?;
                let ordering = match (
                    self.next_tag(&candidate.tags, node, &mut left)?,
                    self.next_tag(&previous.tags, node, &mut right)?,
                ) {
                    (Some(left), Some(right)) => {
                        let ordering = left.span.len().cmp(&right.span.len());
                        if self.regex.nodes[node].shortest {
                            ordering.reverse()
                        } else {
                            ordering
                        }
                    }
                    (Some(_), None) => Ordering::Greater,
                    (None, Some(_)) => Ordering::Less,
                    (None, None) => break,
                };
                if ordering != Ordering::Equal {
                    return Ok(ordering == Ordering::Greater);
                }
            }
        }
        Ok(false)
    }

    fn next_tag(
        &mut self,
        tags: &[Tag],
        node: usize,
        offset: &mut usize,
    ) -> Result<Option<Tag>, QueryError> {
        while let Some(tag) = tags.get(*offset).copied() {
            self.tick()?;
            *offset += 1;
            if tag.node == node {
                return Ok(Some(tag));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
#[path = "regex_constraint_tests.rs"]
mod tests;
