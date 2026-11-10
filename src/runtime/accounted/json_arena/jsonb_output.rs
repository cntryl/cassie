//! Canonical JSONB output from a completely parsed isolated arena.
use crate::app::CassieError;
use crate::runtime::accounted::compact_json::{OwnedJsonText, Progress as SinkProgress, Sink};
use crate::runtime::QueryExecutionControls;

use super::super::directory::Directory;
use super::jsonb_number::{format_number_with_observer, FormatError, Progress as NumberProgress};
use super::jsonb_order::{sorted, Order};
use super::{Forest, Kind, NodeId};

#[cfg(test)]
#[path = "jsonb_output_tests.rs"]
mod tests;

#[derive(Debug)]
pub(in crate::runtime::accounted::json_arena) enum SerializeError {
    NumericOverflow,
    Nul,
    Control(CassieError),
}

impl From<CassieError> for SerializeError {
    fn from(error: CassieError) -> Self {
        Self::Control(error)
    }
}

#[cfg(test)]
impl SerializeError {
    fn sqlstate(&self) -> &'static str {
        match self {
            Self::NumericOverflow => "22003",
            Self::Nul => "22P05",
            Self::Control(_) => "",
        }
    }
    fn message(&self) -> &'static str {
        match self {
            Self::NumericOverflow => "value overflows numeric format",
            Self::Nul => "unsupported Unicode escape sequence: \\u0000",
            Self::Control(_) => "",
        }
    }
}

enum Frame {
    Value(NodeId),
    Array {
        node: NodeId,
        next: usize,
    },
    Object {
        node: NodeId,
        order: Order,
        next: usize,
    },
}

#[derive(Clone, Copy)]
enum OutputProgress {
    SortCompare,
    StringScan,
    SinkScan,
    SinkCopy,
    NumericScan,
    NumericOutput,
}

struct Stack {
    frames: Directory<Frame>,
    depth: usize,
    allocated: usize,
}

impl Stack {
    fn new(controls: &QueryExecutionControls) -> Result<Self, CassieError> {
        Ok(Self {
            frames: Directory::new(controls)?,
            depth: 0,
            allocated: 0,
        })
    }
    fn push(&mut self, frame: Frame) -> Result<(), CassieError> {
        if self.depth == self.allocated {
            self.frames.push(frame)?;
            self.allocated += 1;
        } else {
            self.frames
                .put_empty(self.depth, frame)
                .map_err(|_| CassieError::Execution("occupied JSONB output frame".into()))?;
        }
        self.depth += 1;
        Ok(())
    }
    fn pop(&mut self) -> Option<Frame> {
        if self.depth == 0 {
            return None;
        }
        self.depth -= 1;
        self.frames.take(self.depth)
    }
}

pub(in crate::runtime::accounted::json_arena) fn serialize(
    forest: &Forest,
    root: NodeId,
) -> Result<OwnedJsonText, SerializeError> {
    serialize_with_observer(forest, root, |_| {})
}

#[cfg(test)]
fn serialize_observed(
    forest: &Forest,
    root: NodeId,
    observer: impl FnMut(OutputProgress),
) -> Result<OwnedJsonText, SerializeError> {
    serialize_with_observer(forest, root, observer)
}

fn serialize_with_observer(
    forest: &Forest,
    root: NodeId,
    mut observer: impl FnMut(OutputProgress),
) -> Result<OwnedJsonText, SerializeError> {
    let controls = &forest.controls;
    let mut sink = Sink::for_arena(controls)?;
    let mut stack = Stack::new(controls)?;
    stack.push(Frame::Value(root))?;
    while let Some(frame) = stack.pop() {
        controls.check_interruption()?;
        match frame {
            Frame::Value(id) => match &forest.nodes.get(id)?.kind {
                Kind::Scalar => {
                    sink.write("null", &mut |event| sink_event(event, &mut observer))?;
                }
                Kind::Boolean(value) => sink
                    .write(if *value { "true" } else { "false" }, &mut |event| {
                        sink_event(event, &mut observer);
                    })?,
                Kind::Text(text) => quoted(&mut sink, text.as_str(), controls, &mut observer)?,
                Kind::Number(number) => {
                    let text = format_number_with_observer(number.as_str(), controls, |progress| {
                        observer(match progress {
                            NumberProgress::Scan => OutputProgress::NumericScan,
                            NumberProgress::Output => OutputProgress::NumericOutput,
                        });
                    })
                    .map_err(|error| match error {
                        FormatError::Overflow => SerializeError::NumericOverflow,
                        FormatError::Control(error) => SerializeError::Control(error),
                    })?;
                    sink.write(text.as_str(), &mut |event| sink_event(event, &mut observer))?;
                }
                Kind::Array(_) => {
                    sink.write("[", &mut |event| sink_event(event, &mut observer))?;
                    stack.push(Frame::Array { node: id, next: 0 })?;
                }
                Kind::Object(index) => {
                    sink.write("{", &mut |event| sink_event(event, &mut observer))?;
                    let order = sorted(index, controls, &mut || {
                        observer(OutputProgress::SortCompare);
                    })?;
                    stack.push(Frame::Object {
                        node: id,
                        order,
                        next: 0,
                    })?;
                }
            },
            Frame::Array { node, next } => {
                let Kind::Array(index) = &forest.nodes.get(node)?.kind else {
                    unreachable!("array frame")
                };
                if let Some(child) = index.get(next) {
                    if next != 0 {
                        sink.write(", ", &mut |event| sink_event(event, &mut observer))?;
                    }
                    let following = next.checked_add(1).ok_or_else(|| {
                        CassieError::ResourceLimit("JSONB array position overflow".into())
                    })?;
                    stack.push(Frame::Array {
                        node,
                        next: following,
                    })?;
                    stack.push(Frame::Value(child))?;
                } else {
                    sink.write("]", &mut |event| sink_event(event, &mut observer))?;
                }
            }
            Frame::Object { node, order, next } => {
                let Kind::Object(index) = &forest.nodes.get(node)?.kind else {
                    unreachable!("object frame")
                };
                if let Some(position) = order.get(next) {
                    if next != 0 {
                        sink.write(", ", &mut |event| sink_event(event, &mut observer))?;
                    }
                    let (key, child) = index.item(position);
                    quoted(&mut sink, key, controls, &mut observer)?;
                    sink.write(": ", &mut |event| sink_event(event, &mut observer))?;
                    let following = next.checked_add(1).ok_or_else(|| {
                        CassieError::ResourceLimit("JSONB object position overflow".into())
                    })?;
                    stack.push(Frame::Object {
                        node,
                        order,
                        next: following,
                    })?;
                    stack.push(Frame::Value(child))?;
                } else {
                    sink.write("}", &mut |event| sink_event(event, &mut observer))?;
                }
            }
        }
    }
    Ok(sink.finish_output())
}

fn quoted(
    sink: &mut Sink<'_>,
    text: &str,
    controls: &QueryExecutionControls,
    observer: &mut impl FnMut(OutputProgress),
) -> Result<(), SerializeError> {
    for chunk in text.as_bytes().chunks(1024) {
        controls.check_interruption()?;
        if chunk.contains(&0) {
            return Err(SerializeError::Nul);
        }
        observer(OutputProgress::StringScan);
        controls.check_interruption()?;
    }
    sink.quoted(text, &mut |event| sink_event(event, observer))?;
    Ok(())
}

fn sink_event(event: SinkProgress, observer: &mut impl FnMut(OutputProgress)) {
    match event {
        SinkProgress::Scan => observer(OutputProgress::SinkScan),
        SinkProgress::Copy => observer(OutputProgress::SinkCopy),
        SinkProgress::FrameMove => unreachable!("sink never moves traversal frames"),
    }
}
