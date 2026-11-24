//! Finite ARE constraints compiled from a published AST into Cassie-owned state.

use std::ops::Range;

use fancy_regex::{Assertion, Expr, LookAround};
use regex_automata::{util::syntax, Input, Match, Span};

use super::{
    bounded, check_interruption, invalid_regular_expression, newline_classes,
    pattern_memory::{syntax_scratch, PatternText},
    resource_limit, QueryError,
};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

#[path = "regex_constraint_memory.rs"]
mod memory;
#[path = "regex_constraint_walk.rs"]
mod walk;

#[path = "regex_constraint_source.rs"]
mod source;

use memory::Directory;

pub(super) struct Regex {
    nodes: Directory<Node>,
    groups: usize,
    _syntax_memory: Option<QueryMemoryReservation>,
}

struct Node {
    kind: Kind,
    captures: Range<usize>,
    shortest: bool,
    subtree_end: usize,
}

enum Kind {
    Empty,
    Atom(Directory<bounded::Regex>),
    Concat(Directory<usize>),
    Alt(Directory<usize>),
    Group {
        child: usize,
        slot: usize,
    },
    Repeat {
        child: usize,
        lo: usize,
        hi: usize,
        split_final: bool,
    },
    Look {
        child: usize,
        kind: LookAround,
    },
    Backref {
        slot: usize,
        casei: bool,
    },
}

pub(super) struct Captures {
    spans: Directory<Option<Span>>,
}

impl Captures {
    pub(super) fn get_match(&self) -> Option<Match> {
        self.spans
            .first()
            .copied()
            .flatten()
            .map(|span| Match::must(0, span))
    }
    pub(super) fn get_group(&self, group: usize) -> Option<Span> {
        self.spans.get(group).copied().flatten()
    }
}

impl Regex {
    pub(super) fn build(
        pattern: PatternText,
        advanced: bool,
        syntax: syntax::Config,
        exclude_newline: bool,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<Option<Self>, QueryError> {
        source::validate(
            pattern.as_str(),
            syntax.get_ignore_whitespace(),
            advanced,
            controls,
        )?;
        let pattern = prepare(pattern, advanced, syntax, controls)?;
        let syntax_memory = syntax_scratch(pattern.as_str().len(), controls)?;
        let Ok(tree) = Expr::parse_tree(pattern.as_str()) else {
            return Ok(None);
        };
        if !contains_constraint(&tree.expr) {
            return Ok(None);
        }
        if !tree.named_groups.is_empty() || tree.contains_subroutines {
            return Err(invalid_regular_expression(
                "unselected named reference or recursive syntax",
            ));
        }
        let mut compiler = Compiler {
            nodes: Directory::new(),
            groups: 0,
            closed: Directory::new(),
            syntax,
            exclude_newline,
            advanced,
            controls,
        };
        compiler.node(&tree.expr, false, 0)?;
        let result = Self {
            nodes: compiler.nodes,
            groups: compiler.groups + 1,
            _syntax_memory: syntax_memory,
        };
        // AST is gone before its workspace lease becomes the retained program envelope.
        drop(tree);
        check_interruption(controls)?;
        Ok(Some(result))
    }

    pub(super) fn create_captures(
        &self,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<Captures, QueryError> {
        let mut spans = Directory::with_capacity(self.groups, controls)?;
        spans.values.resize(self.groups, None);
        Ok(Captures { spans })
    }

    pub(super) fn find(
        &self,
        input: &Input<'_>,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<Option<Match>, QueryError> {
        self.search(input, controls)
            .map(|state| state.and_then(|state| state.captures[0].map(|span| Match::must(0, span))))
    }

    pub(super) fn search_captures(
        &self,
        input: &Input<'_>,
        captures: &mut Captures,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<(), QueryError> {
        let state = self.search(input, controls)?;
        captures.spans.fill(None);
        if let Some(state) = state {
            captures.spans.copy_from_slice(&state.captures);
        }
        Ok(())
    }
}

fn contains_constraint(expr: &Expr) -> bool {
    matches!(expr, Expr::LookAround(..) | Expr::Backref { .. })
        || expr.children_iter().any(contains_constraint)
}

struct Compiler<'a> {
    nodes: Directory<Node>,
    groups: usize,
    closed: Directory<bool>,
    syntax: syntax::Config,
    exclude_newline: bool,
    advanced: bool,
    controls: Option<&'a QueryExecutionControls>,
}

impl Compiler<'_> {
    fn node(&mut self, expr: &Expr, in_look: bool, depth: usize) -> Result<usize, QueryError> {
        check_interruption(self.controls)?;
        if depth >= 64 {
            return Err(invalid_regular_expression(
                "constraint expression nesting limit exceeded",
            ));
        }
        let id = self.nodes.len();
        let first = self.groups + 1;
        self.nodes.push(
            Node {
                kind: Kind::Empty,
                captures: first..first,
                shortest: false,
                subtree_end: id + 1,
            },
            self.controls,
        )?;
        let kind = self.kind(expr, in_look, depth)?;
        self.nodes[id] = Node {
            kind,
            captures: first..self.groups + 1,
            shortest: greediness(expr) == Some(false),
            subtree_end: self.nodes.len(),
        };
        Ok(id)
    }

    fn kind(&mut self, expr: &Expr, in_look: bool, depth: usize) -> Result<Kind, QueryError> {
        Ok(match expr {
            Expr::Empty => Kind::Empty,
            Expr::Concat(children) | Expr::Alt(children) => {
                let ids = self.children(children, in_look, depth)?;
                if matches!(expr, Expr::Concat(_)) {
                    Kind::Concat(ids)
                } else {
                    Kind::Alt(ids)
                }
            }
            Expr::Group(child) if in_look => Kind::Concat({
                let mut ids = Directory::with_capacity(1, self.controls)?;
                ids.push(self.node(child, true, depth + 1)?, self.controls)?;
                ids
            }),
            Expr::Group(child) => {
                self.groups = self
                    .groups
                    .checked_add(1)
                    .ok_or_else(|| resource_limit("regex capture count overflow"))?;
                let slot = self.groups;
                self.closed.push(false, self.controls)?;
                let child = self.node(child, false, depth + 1)?;
                self.closed[slot - 1] = true;
                Kind::Group { child, slot }
            }
            Expr::Repeat { child, lo, hi, .. } => Kind::Repeat {
                child: self.node(child, in_look, depth + 1)?,
                lo: *lo,
                hi: *hi,
                split_final: *lo > 0 && !contains_backref(child),
            },
            Expr::LookAround(child, kind) if self.advanced => Kind::Look {
                child: self.node(child, true, depth + 1)?,
                kind: *kind,
            },
            Expr::Backref { group, casei } => {
                if in_look || *group == 0 || !self.closed.get(group - 1).copied().unwrap_or(false) {
                    return Err(invalid_regular_expression(
                        "backreference must name a previously closed external capture",
                    ));
                }
                Kind::Backref {
                    slot: *group,
                    casei: *casei,
                }
            }
            Expr::Literal { val, casei } => self.literal(val, *casei)?,
            Expr::Delegate { inner, casei } => self.delegate(inner, *casei)?,
            Expr::Any {
                newline,
                crlf: false,
            } => self.atom(
                ".",
                self.syntax
                    .dot_matches_new_line(*newline)
                    .ignore_whitespace(false),
            )?,
            Expr::Assertion(assertion) => self.assertion(*assertion)?,
            _ => {
                return Err(invalid_regular_expression(
                    "unselected constraint expression syntax",
                ))
            }
        })
    }

    fn children(
        &mut self,
        children: &[Expr],
        in_look: bool,
        depth: usize,
    ) -> Result<Directory<usize>, QueryError> {
        let mut ids = Directory::with_capacity(children.len(), self.controls)?;
        for child in children {
            ids.push(self.node(child, in_look, depth + 1)?, self.controls)?;
        }
        Ok(ids)
    }

    fn literal(&self, value: &str, casei: bool) -> Result<Kind, QueryError> {
        let escaped = escape(value, self.controls)?;
        self.atom(
            escaped.as_str(),
            self.syntax.case_insensitive(casei).ignore_whitespace(false),
        )
    }

    fn delegate(&self, source: &str, casei: bool) -> Result<Kind, QueryError> {
        let source = if self.exclude_newline {
            newline_classes::exclude_newlines_owned(
                source,
                self.syntax.get_ignore_whitespace(),
                self.controls,
            )?
        } else {
            PatternText::copy(source, self.controls)?
        };
        self.atom(source.as_str(), self.syntax.case_insensitive(casei))
    }

    fn assertion(&self, assertion: Assertion) -> Result<Kind, QueryError> {
        let (pattern, multiline) = match assertion {
            Assertion::StartText => (r"\A", false),
            Assertion::EndText => (r"\z", false),
            Assertion::StartLine { crlf: false } => ("^", true),
            Assertion::EndLine { crlf: false } => ("$", true),
            Assertion::LeftWordBoundary => (r"\b{start}", false),
            Assertion::RightWordBoundary => (r"\b{end}", false),
            Assertion::WordBoundary => (r"\b", false),
            Assertion::NotWordBoundary => (r"\B", false),
            _ => {
                return Err(invalid_regular_expression(
                    "unselected constraint assertion",
                ))
            }
        };
        self.atom(
            pattern,
            self.syntax.multi_line(multiline).ignore_whitespace(false),
        )
    }

    fn atom(&self, pattern: &str, syntax: syntax::Config) -> Result<Kind, QueryError> {
        let mut atom = Directory::with_capacity(1, self.controls)?;
        atom.push(
            bounded::Regex::build(pattern, syntax, None, self.controls)?,
            self.controls,
        )?;
        Ok(Kind::Atom(atom))
    }
}

fn greediness(expr: &Expr) -> Option<bool> {
    match expr {
        Expr::Group(child) => greediness(child),
        Expr::Concat(children) => children.iter().find_map(greediness),
        Expr::Alt(children) if children.len() > 1 => Some(true),
        Expr::Alt(children) => children.first().and_then(greediness),
        Expr::Repeat {
            child,
            lo,
            hi,
            greedy,
        } => {
            if lo == hi {
                greediness(child)
            } else {
                Some(*greedy)
            }
        }
        _ => None,
    }
}

fn contains_backref(expr: &Expr) -> bool {
    matches!(expr, Expr::Backref { .. }) || expr.children_iter().any(contains_backref)
}

fn escape(
    value: &str,
    controls: Option<&QueryExecutionControls>,
) -> Result<PatternText, QueryError> {
    let capacity = value
        .len()
        .checked_mul(2)
        .ok_or_else(|| resource_limit("constraint literal overflow"))?;
    PatternText::build(capacity, controls, |writer| {
        for character in value.chars() {
            check_interruption(controls)?;
            if regex_syntax::is_meta_character(character) {
                writer.push('\\')?;
            }
            writer.push(character)?;
        }
        Ok(())
    })
}

fn prepare(
    pattern: PatternText,
    advanced: bool,
    syntax: syntax::Config,
    controls: Option<&QueryExecutionControls>,
) -> Result<PatternText, QueryError> {
    let capacity = pattern
        .as_str()
        .len()
        .checked_mul(8)
        .and_then(|bytes| bytes.checked_add(12))
        .ok_or_else(|| resource_limit("constraint preparation size overflow"))?;
    pattern.transform(capacity, controls, |input, writer| {
        writer.push_str("(?")?;
        if syntax.get_case_insensitive() {
            writer.push('i')?;
        }
        if syntax.get_multi_line() {
            writer.push('m')?;
        }
        if syntax.get_dot_matches_new_line() {
            writer.push('s')?;
        }
        if syntax.get_ignore_whitespace() {
            writer.push('x')?;
        }
        writer.push_str(":")?;
        let mut offset = 0;
        while offset < input.len() {
            check_interruption(controls)?;
            let suffix = &input[offset..];
            if suffix.starts_with("[[:<:]]") || suffix.starts_with("[[:>:]]") {
                writer.push_str(if suffix.starts_with("[[:<:]]") {
                    r"\b{start}"
                } else {
                    r"\b{end}"
                })?;
                offset += 7;
                continue;
            }
            let character = suffix.chars().next().expect("source scalar boundary");
            offset += character.len_utf8();
            if character != '\\' {
                writer.push(character)?;
                continue;
            }
            let Some(next) = input[offset..].chars().next() else {
                writer.push('\\')?;
                break;
            };
            offset += next.len_utf8();
            if advanced {
                let replacement = match next {
                    'm' => Some(r"\b{start}"),
                    'M' => Some(r"\b{end}"),
                    'y' => Some(r"\b"),
                    'Y' => Some(r"\B"),
                    'Z' => Some(r"\z"),
                    'b' => Some(r"\x08"),
                    'B' => Some(r"\\"),
                    _ => None,
                };
                if let Some(replacement) = replacement {
                    writer.push_str(replacement)?;
                    continue;
                }
            } else if matches!(next, 'b' | 'B' | 'm' | 'M' | 'y' | 'Y' | 'Z') {
                writer.push(next)?;
                continue;
            }
            writer.push('\\')?;
            writer.push(next)?;
        }
        writer.push(')')
    })
}
