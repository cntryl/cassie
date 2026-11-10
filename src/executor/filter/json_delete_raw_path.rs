//! Bottom-up raw path deletion preserving child-before-parent formatting.
use crate::app::CassieError;
use crate::runtime::accounted::compact_json::OwnedJsonText;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

use super::super::object::{self, Fields};
use super::{array_count, array_scanner, render_frames, ArraySkip, Frame, KeySkip, RenderError};

#[path = "json_delete_path_index_controlled.rs"]
mod index;

enum Ancestor<'a> {
    Object { fields: Fields<'a>, key: &'a str },
    Array { source: &'a str, index: usize },
}

#[derive(Clone, Copy)]
enum PathView<'a> {
    Owned(&'a [Option<String>]),
    Native(&'a [crate::types::Value]),
    Arena(&'a crate::runtime::accounted::json_arena::OwnedTextArray),
}

enum PathElement<'a> {
    Missing,
    Null,
    Value(&'a str),
}

impl<'a> PathView<'a> {
    fn len(self) -> usize {
        match self {
            Self::Owned(values) => values.len(),
            Self::Native(values) => values.len(),
            Self::Arena(values) => values.len(),
        }
    }

    fn get(self, index: usize) -> Result<PathElement<'a>, CassieError> {
        match self {
            Self::Owned(values) => Ok(match values.get(index) {
                None => PathElement::Missing,
                Some(None) => PathElement::Null,
                Some(Some(value)) => PathElement::Value(value),
            }),
            Self::Native(values) => Ok(match values.get(index) {
                None => PathElement::Missing,
                Some(crate::types::Value::String(text)) => PathElement::Value(text),
                Some(crate::types::Value::Null) => PathElement::Null,
                Some(_) => unreachable!("native path elements were validated"),
            }),
            Self::Arena(values) if index < values.len() => Ok(match values.get(index)? {
                Some(value) => PathElement::Value(value),
                None => PathElement::Null,
            }),
            Self::Arena(_) => Ok(PathElement::Missing),
        }
    }
}

struct Ancestors<'a> {
    values: Vec<Ancestor<'a>>,
    memory: QueryMemoryReservation,
    controls: &'a QueryExecutionControls,
}

struct Replacement<'a> {
    values: Vec<Ancestor<'a>>,
    memory: QueryMemoryReservation,
}

impl<'a> Ancestors<'a> {
    fn new(controls: &'a QueryExecutionControls) -> Result<Self, CassieError> {
        Ok(Self {
            values: Vec::new(),
            memory: controls.reserve_query_memory(0)?,
            controls,
        })
    }

    fn push(&mut self, value: Ancestor<'a>, observe: &mut impl FnMut()) -> Result<(), CassieError> {
        if self.values.len() == self.values.capacity() {
            let capacity = self.values.len().saturating_mul(2).max(4);
            let bytes = capacity
                .checked_mul(std::mem::size_of::<Ancestor<'_>>())
                .ok_or_else(|| CassieError::ResourceLimit("JSON path stack overflow".into()))?;
            let mut replacement = Replacement {
                values: Vec::new(),
                memory: self.controls.reserve_query_memory(bytes)?,
            };
            replacement
                .values
                .try_reserve_exact(capacity)
                .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
            if replacement.values.capacity() > capacity {
                return Err(CassieError::ResourceLimit(
                    "JSON path stack exceeded its admission".into(),
                ));
            }
            while let Some(old) = self.values.pop() {
                self.controls.check_interruption()?;
                replacement.values.push(old);
                observe();
                self.controls.check_interruption()?;
            }
            let mut left = 0;
            let mut right = replacement.values.len();
            while left < right {
                self.controls.check_interruption()?;
                right -= 1;
                if left >= right {
                    break;
                }
                replacement.values.swap(left, right);
                left += 1;
            }
            let Replacement { values, memory } = replacement;
            let old_values = std::mem::replace(&mut self.values, values);
            let old_memory = std::mem::replace(&mut self.memory, memory);
            drop(old_values);
            drop(old_memory);
        }
        self.values.push(value);
        Ok(())
    }
}

pub(in crate::executor::filter) fn render_path<'a>(
    source: &'a str,
    path: &'a [Option<String>],
    controls: &'a QueryExecutionControls,
) -> Result<OwnedJsonText, RenderError> {
    render_path_observing(source, path, controls, || {})
}

pub(super) fn render_path_observing<'a>(
    source: &'a str,
    path: &'a [Option<String>],
    controls: &'a QueryExecutionControls,
    observe: impl FnMut(),
) -> Result<OwnedJsonText, RenderError> {
    render_view(source, PathView::Owned(path), controls, observe)
}

pub(in crate::executor::filter) fn render_native_path<'a>(
    source: &'a str,
    path: &'a [crate::types::Value],
    controls: &'a QueryExecutionControls,
) -> Result<OwnedJsonText, RenderError> {
    for value in path {
        controls.check_interruption()?;
        if !matches!(
            value,
            crate::types::Value::String(_) | crate::types::Value::Null
        ) {
            return Err(RenderError::Path(
                super::super::super::unsupported_subtraction_operand(),
            ));
        }
    }
    render_view(source, PathView::Native(path), controls, || {})
}

pub(in crate::executor::filter) fn render_arena_path<'a>(
    source: &'a str,
    path: &'a crate::runtime::accounted::json_arena::OwnedTextArray,
    controls: &'a QueryExecutionControls,
) -> Result<OwnedJsonText, RenderError> {
    render_view(source, PathView::Arena(path), controls, || {})
}

fn render_view<'a>(
    source: &'a str,
    path: PathView<'a>,
    controls: &'a QueryExecutionControls,
    mut observe: impl FnMut(),
) -> Result<OwnedJsonText, RenderError> {
    controls.check_interruption()?;
    let mut prefix = super::super::Scanner {
        input: source.as_bytes(),
        position: 0,
        line: 1,
        column: 0,
        controls,
        observe: super::ignore_scan as fn(super::ScanProgress),
    };
    prefix.whitespace()?;
    if !matches!(prefix.peek(), Some(b'{' | b'[')) {
        return Err(RenderError::Path(super::super::super::invalid_parameter(
            "cannot delete path in scalar",
        )));
    }
    let span = super::super::scan(source, controls)?;
    let root = &source[span];
    if is_empty_container(root, controls)? {
        return render_frames(Frame::Value(root), controls, |_| {});
    }
    let (output, ancestors) = walk_path(root, path, controls, &mut observe)?;
    rebuild(output, ancestors, controls)
}

enum PathStep<'a> {
    Descend(&'a str),
    Finish(OwnedJsonText),
}

fn walk_path<'a>(
    root: &'a str,
    path: PathView<'a>,
    controls: &'a QueryExecutionControls,
    observe: &mut impl FnMut(),
) -> Result<(OwnedJsonText, Ancestors<'a>), RenderError> {
    let mut ancestors = Ancestors::new(controls)?;
    let mut current = root;
    let mut position = 0;
    loop {
        controls.check_interruption()?;
        let head = match path.get(position)? {
            PathElement::Missing => {
                let output = render_frames(Frame::Value(current), controls, |_| {})?;
                return Ok((output, ancestors));
            }
            PathElement::Null => {
                return Err(RenderError::Path(super::super::super::type_input_error(
                    "22004",
                    format!("path element at position {} is null", position + 1),
                )));
            }
            PathElement::Value(head) => head,
        };
        let last = position + 1 == path.len();
        let step = match current.as_bytes().first().copied() {
            Some(b'{') => object_step(current, head, last, controls, &mut ancestors, observe)?,
            Some(b'[') => array_step(
                current,
                head,
                last,
                position,
                controls,
                &mut ancestors,
                observe,
            )?,
            _ => PathStep::Finish(render_frames(Frame::Value(current), controls, |_| {})?),
        };
        match step {
            PathStep::Descend(child) => current = child,
            PathStep::Finish(output) => return Ok((output, ancestors)),
        }
        position += 1;
    }
}

fn is_empty_container(
    source: &str,
    controls: &QueryExecutionControls,
) -> Result<bool, RenderError> {
    let mut probe = super::super::Scanner {
        input: source.as_bytes(),
        position: 0,
        line: 1,
        column: 0,
        controls,
        observe: super::ignore_scan as fn(super::ScanProgress),
    };
    let opener = probe.next()?;
    probe.whitespace()?;
    Ok(matches!(
        (opener, probe.peek()),
        (Some(b'{'), Some(b'}')) | (Some(b'['), Some(b']'))
    ))
}

fn object_step<'a>(
    current: &'a str,
    head: &'a str,
    last: bool,
    controls: &'a QueryExecutionControls,
    ancestors: &mut Ancestors<'a>,
    observe: &mut impl FnMut(),
) -> Result<PathStep<'a>, RenderError> {
    let fields = object::parse(current, controls)?;
    if last {
        let fields = fields.into_ordered(controls)?;
        return Ok(PathStep::Finish(render_frames(
            Frame::Object {
                fields,
                next: 0,
                emitted: false,
                skip: Some(KeySkip::One(head)),
                replace: None,
            },
            controls,
            |_| {},
        )?));
    }
    if let Some(child) = fields.last_value(head, controls)? {
        ancestors.push(Ancestor::Object { fields, key: head }, observe)?;
        Ok(PathStep::Descend(child))
    } else {
        let fields = fields.into_ordered(controls)?;
        Ok(PathStep::Finish(render_frames(
            Frame::Object {
                fields,
                next: 0,
                emitted: false,
                skip: None,
                replace: None,
            },
            controls,
            |_| {},
        )?))
    }
}

fn array_step<'a>(
    current: &'a str,
    head: &'a str,
    last: bool,
    position: usize,
    controls: &'a QueryExecutionControls,
    ancestors: &mut Ancestors<'a>,
    observe: &mut impl FnMut(),
) -> Result<PathStep<'a>, RenderError> {
    let count = array_count(current, controls)?;
    let index = index::parse(head, count, position + 1, controls).map_err(RenderError::Path)?;
    let Some(index) = index else {
        return Ok(PathStep::Finish(render_frames(
            Frame::Value(current),
            controls,
            |_| {},
        )?));
    };
    let mut scanner = array_scanner(current, controls)?;
    let _consumed = scanner.next()?;
    if last {
        return Ok(PathStep::Finish(render_frames(
            Frame::Array {
                source: current,
                scanner,
                next: 0,
                emitted: false,
                skip: Some(ArraySkip::Index(index)),
                replace: None,
            },
            controls,
            |_| {},
        )?));
    }
    let child = array_element(current, index, controls)?;
    ancestors.push(
        Ancestor::Array {
            source: current,
            index,
        },
        observe,
    )?;
    Ok(PathStep::Descend(child))
}

fn rebuild<'a>(
    mut output: OwnedJsonText,
    mut ancestors: Ancestors<'a>,
    controls: &'a QueryExecutionControls,
) -> Result<OwnedJsonText, RenderError> {
    while let Some(ancestor) = ancestors.values.pop() {
        controls.check_interruption()?;
        let replacement = output.as_str();
        let next = match ancestor {
            Ancestor::Object { fields, key } => {
                let fields = fields.into_ordered(controls)?;
                render_frames(
                    Frame::Object {
                        fields,
                        next: 0,
                        emitted: false,
                        skip: None,
                        replace: Some((key, replacement)),
                    },
                    controls,
                    |_| {},
                )?
            }
            Ancestor::Array { source, index } => {
                let mut scanner = array_scanner(source, controls)?;
                let _consumed = scanner.next()?;
                render_frames(
                    Frame::Array {
                        source,
                        scanner,
                        next: 0,
                        emitted: false,
                        skip: None,
                        replace: Some((index, replacement)),
                    },
                    controls,
                    |_| {},
                )?
            }
        };
        drop(output);
        output = next;
    }
    Ok(output)
}

fn array_element<'a>(
    source: &'a str,
    index: usize,
    controls: &QueryExecutionControls,
) -> Result<&'a str, RenderError> {
    let mut scanner = array_scanner(source, controls)?;
    let _consumed = scanner.next()?;
    for current in 0..=index {
        scanner.whitespace()?;
        if current > 0 {
            let _consumed = scanner.next()?;
        }
        let span = scanner.value_span()?;
        if current == index {
            return Ok(&source[span]);
        }
    }
    Err(RenderError::InvalidValue)
}
