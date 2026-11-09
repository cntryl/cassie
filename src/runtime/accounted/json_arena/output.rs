//! Compact serialization of the isolated, immutable JSON arena forest.
use crate::app::CassieError;
use crate::runtime::accounted::compact_json::{OwnedJsonText, Progress as SinkProgress, Sink};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

use super::{Forest, Kind, NodeId};

#[cfg(test)]
#[path = "output_tests.rs"]
mod tests;

#[derive(Clone, Copy)]
enum Frame {
    Value(NodeId),
    Array {
        node: NodeId,
        next: usize,
    },
    Object {
        node: NodeId,
        next: Option<usize>,
        emitted: bool,
    },
}

#[derive(Clone, Copy)]
enum OutputProgress {
    SinkScan,
    SinkCopy,
    FrameCopy,
    KeyCompare,
}

struct Stack<'a> {
    frames: Vec<Frame>,
    memory: QueryMemoryReservation,
    controls: &'a QueryExecutionControls,
}

impl<'a> Stack<'a> {
    fn new(controls: &'a QueryExecutionControls) -> Result<Self, CassieError> {
        Ok(Self {
            frames: Vec::new(),
            memory: controls.reserve_query_memory(0)?,
            controls,
        })
    }

    fn push(
        &mut self,
        frame: Frame,
        observer: &mut impl FnMut(OutputProgress),
    ) -> Result<(), CassieError> {
        self.controls.check_interruption()?;
        if self.frames.len() == self.frames.capacity() {
            let capacity = self
                .frames
                .len()
                .checked_add(1)
                .and_then(|length| length.max(4).checked_next_power_of_two())
                .ok_or_else(|| CassieError::ResourceLimit("JSON output stack overflow".into()))?;
            let memory;
            let mut replacement;
            (replacement, memory) =
                crate::runtime::owned_memory::try_reserve_vec(capacity, &self.memory)?;
            for old in self.frames.iter().copied() {
                self.controls.check_interruption()?;
                replacement.push(old);
                observer(OutputProgress::FrameCopy);
                self.controls.check_interruption()?;
            }
            let old = std::mem::replace(&mut self.frames, replacement);
            let old_memory = std::mem::replace(&mut self.memory, memory);
            drop(old);
            drop(old_memory);
        }
        self.frames.push(frame);
        Ok(())
    }
}

fn serialize(forest: &Forest, root: NodeId) -> Result<OwnedJsonText, CassieError> {
    serialize_with_observer(forest, root, |_| {})
}

#[cfg(test)]
fn serialize_observed(
    forest: &Forest,
    root: NodeId,
    observer: impl FnMut(OutputProgress),
) -> Result<OwnedJsonText, CassieError> {
    serialize_with_observer(forest, root, observer)
}

fn serialize_with_observer(
    forest: &Forest,
    root: NodeId,
    mut observer: impl FnMut(OutputProgress),
) -> Result<OwnedJsonText, CassieError> {
    let controls = &forest.controls;
    controls.check_interruption()?;
    let mut sink = Sink::for_arena(controls)?;
    let mut stack = Stack::new(controls)?;
    stack.push(Frame::Value(root), &mut observer)?;
    while let Some(frame) = stack.frames.pop() {
        controls.check_interruption()?;
        match frame {
            Frame::Value(id) => match &forest.nodes.get(id)?.kind {
                Kind::Scalar => {
                    sink.write("null", &mut |event| sink_event(event, &mut observer))?
                }
                Kind::Boolean(value) => sink
                    .write(if *value { "true" } else { "false" }, &mut |event| {
                        sink_event(event, &mut observer)
                    })?,
                Kind::Text(value) => sink.quoted(value.as_str(), &mut |event| {
                    sink_event(event, &mut observer)
                })?,
                Kind::Number(value) => sink.write(value.as_str(), &mut |event| {
                    sink_event(event, &mut observer)
                })?,
                Kind::Array(_) => {
                    sink.write("[", &mut |event| sink_event(event, &mut observer))?;
                    stack.push(Frame::Array { node: id, next: 0 }, &mut observer)?;
                }
                Kind::Object(index) => {
                    sink.write("{", &mut |event| sink_event(event, &mut observer))?;
                    let next = index.first_index()?;
                    stack.push(
                        Frame::Object {
                            node: id,
                            next,
                            emitted: false,
                        },
                        &mut observer,
                    )?;
                }
            },
            Frame::Array { node, next } => {
                let Kind::Array(index) = &forest.nodes.get(node)?.kind else {
                    unreachable!("validated array node")
                };
                if let Some(child) = index.get(next) {
                    if next != 0 {
                        sink.write(",", &mut |event| sink_event(event, &mut observer))?;
                    }
                    let following = next.checked_add(1).ok_or_else(|| {
                        CassieError::ResourceLimit("JSON array position overflow".into())
                    })?;
                    stack.push(
                        Frame::Array {
                            node,
                            next: following,
                        },
                        &mut observer,
                    )?;
                    stack.push(Frame::Value(child), &mut observer)?;
                } else {
                    sink.write("]", &mut |event| sink_event(event, &mut observer))?;
                }
            }
            Frame::Object {
                node,
                next,
                emitted,
            } => {
                let Kind::Object(index) = &forest.nodes.get(node)?.kind else {
                    unreachable!("validated object node")
                };
                if let Some(position) = next {
                    if emitted {
                        sink.write(",", &mut |event| sink_event(event, &mut observer))?;
                    }
                    let (key, child) = index.item(position);
                    sink.quoted(key, &mut |event| sink_event(event, &mut observer))?;
                    sink.write(":", &mut |event| sink_event(event, &mut observer))?;
                    let following =
                        index.next_index(position, &mut || observer(OutputProgress::KeyCompare))?;
                    stack.push(
                        Frame::Object {
                            node,
                            next: following,
                            emitted: true,
                        },
                        &mut observer,
                    )?;
                    stack.push(Frame::Value(child), &mut observer)?;
                } else {
                    sink.write("}", &mut |event| sink_event(event, &mut observer))?;
                }
            }
        }
    }
    Ok(sink.finish_output())
}

fn sink_event(event: SinkProgress, observer: &mut impl FnMut(OutputProgress)) {
    match event {
        SinkProgress::Scan => observer(OutputProgress::SinkScan),
        SinkProgress::Copy => observer(OutputProgress::SinkCopy),
        SinkProgress::FrameMove => unreachable!("sink never moves traversal frames"),
    }
}
