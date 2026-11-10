//! Checked traversal of two owned Value-mode forests for scalar JSONB ordering.
use super::super::raw_number::{self, RawNumberError};
use super::{jsonb_order, Forest, Kind, NodeId};
use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use std::cmp::Ordering;

#[cfg(test)]
thread_local! {
    static CANCEL_WORK: std::cell::RefCell<Option<crate::runtime::QueryCancellationHandle>> = const { std::cell::RefCell::new(None) };
    static CANCEL_MOVE: std::cell::RefCell<Option<crate::runtime::QueryCancellationHandle>> = const { std::cell::RefCell::new(None) };
    static CANCEL_REVERSE: std::cell::RefCell<Option<crate::runtime::QueryCancellationHandle>> = const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
pub(crate) fn cancel_comparison_work_once(handle: crate::runtime::QueryCancellationHandle) {
    CANCEL_WORK.with(|slot| *slot.borrow_mut() = Some(handle));
}
#[cfg(test)]
pub(crate) fn cancel_comparison_frame_move_once(handle: crate::runtime::QueryCancellationHandle) {
    CANCEL_MOVE.with(|slot| *slot.borrow_mut() = Some(handle));
}
#[cfg(test)]
pub(crate) fn cancel_comparison_reverse_once(handle: crate::runtime::QueryCancellationHandle) {
    CANCEL_REVERSE.with(|slot| *slot.borrow_mut() = Some(handle));
}
#[cfg(test)]
fn reverse_observed() {
    CANCEL_REVERSE.with(|slot| {
        if let Some(handle) = slot.borrow_mut().take() {
            handle.cancel();
        }
    });
}
#[cfg(not(test))]
fn reverse_observed() {}
#[cfg(test)]
fn work_observed() {
    CANCEL_WORK.with(|slot| {
        if let Some(handle) = slot.borrow_mut().take() {
            handle.cancel();
        }
    });
}
#[cfg(not(test))]
fn work_observed() {}
#[cfg(test)]
fn move_observed() {
    CANCEL_MOVE.with(|slot| {
        if let Some(handle) = slot.borrow_mut().take() {
            handle.cancel();
        }
    });
}
#[cfg(not(test))]
fn move_observed() {}

enum Task {
    Pair(NodeId, NodeId),
    Array {
        left: NodeId,
        right: NodeId,
        index: usize,
    },
    Object {
        left: NodeId,
        right: NodeId,
        index: usize,
        left_order: jsonb_order::Order,
        right_order: jsonb_order::Order,
    },
}
struct Stack {
    tasks: Vec<Task>,
    memory: QueryMemoryReservation,
}
struct Replacement {
    tasks: Vec<Task>,
    memory: QueryMemoryReservation,
}
impl Stack {
    fn new(controls: &QueryExecutionControls) -> Result<Self, CassieError> {
        Ok(Self {
            tasks: Vec::new(),
            memory: controls.reserve_query_memory(0)?,
        })
    }
    fn push(&mut self, task: Task, controls: &QueryExecutionControls) -> Result<(), CassieError> {
        if self.tasks.len() == self.tasks.capacity() {
            let capacity = self
                .tasks
                .len()
                .checked_add(1)
                .and_then(|n| n.max(4).checked_next_power_of_two())
                .ok_or_else(|| {
                    CassieError::ResourceLimit("JSONB comparison stack overflow".into())
                })?;
            let (tasks, memory) =
                crate::runtime::owned_memory::try_reserve_vec(capacity, &self.memory)?;
            let mut next = Replacement { tasks, memory };
            while let Some(previous) = self.tasks.pop() {
                controls.check_interruption()?;
                next.tasks.push(previous);
                move_observed();
                controls.check_interruption()?;
            }
            let length = next.tasks.len();
            for index in 0..length / 2 {
                controls.check_interruption()?;
                next.tasks.swap(index, length - index - 1);
                reverse_observed();
                controls.check_interruption()?;
            }
            let old = std::mem::replace(&mut self.tasks, next.tasks);
            let old_memory = std::mem::replace(&mut self.memory, next.memory);
            drop(old);
            drop(old_memory);
        }
        controls.check_interruption()?;
        self.tasks.push(task);
        Ok(())
    }
}
fn bytes(
    left: &[u8],
    right: &[u8],
    controls: &QueryExecutionControls,
) -> Result<Ordering, CassieError> {
    for (a, b) in left.chunks(1024).zip(right.chunks(1024)) {
        controls.check_interruption()?;
        let result = a.cmp(b);
        work_observed();
        controls.check_interruption()?;
        if !result.is_eq() {
            return Ok(result);
        }
    }
    Ok(left.len().cmp(&right.len()))
}
fn rank(kind: &Kind) -> u8 {
    match kind {
        Kind::Scalar => 1,
        Kind::Text(_) => 2,
        Kind::Number(_) => 3,
        Kind::Boolean(_) => 4,
        Kind::Array(_) => 5,
        Kind::Object(_) => 6,
    }
}
fn parts<'a>(
    text: &'a str,
    controls: &QueryExecutionControls,
) -> Result<(bool, &'a [u8], &'a [u8]), CassieError> {
    let negative = text.starts_with('-');
    let raw = &text.as_bytes()[usize::from(negative)..];
    let mut point = raw.len();
    for (i, chunk) in raw.chunks(1024).enumerate() {
        controls.check_interruption()?;
        if let Some(offset) = chunk.iter().position(|byte| *byte == b'.') {
            point = i * 1024 + offset;
            break;
        }
    }
    Ok((
        negative,
        &raw[..point],
        if point < raw.len() {
            &raw[point + 1..]
        } else {
            &[]
        },
    ))
}
fn number(
    left: &str,
    right: &str,
    controls: &QueryExecutionControls,
) -> Result<Option<Ordering>, CassieError> {
    let left = match raw_number::format(left, controls) {
        Ok(value) => value,
        Err(RawNumberError::Overflow) => return Ok(None),
        Err(RawNumberError::Control(error)) => return Err(error),
    };
    let right = match raw_number::format(right, controls) {
        Ok(value) => value,
        Err(RawNumberError::Overflow) => return Ok(None),
        Err(RawNumberError::Control(error)) => return Err(error),
    };
    let (negative_left, integer_left, fraction_left) = parts(left.get(), controls)?;
    let (negative_right, integer_right, fraction_right) = parts(right.get(), controls)?;
    if negative_left != negative_right {
        return Ok(Some(if negative_left {
            Ordering::Less
        } else {
            Ordering::Greater
        }));
    }
    let mut order = integer_left.len().cmp(&integer_right.len());
    if order.is_eq() {
        order = bytes(integer_left, integer_right, controls)?;
    }
    if order.is_eq() {
        for index in 0..fraction_left.len().max(fraction_right.len()) {
            if index.is_multiple_of(1024) {
                controls.check_interruption()?;
            }
            order = fraction_left
                .get(index)
                .copied()
                .unwrap_or(b'0')
                .cmp(&fraction_right.get(index).copied().unwrap_or(b'0'));
            if !order.is_eq() {
                break;
            }
        }
    }
    Ok(Some(if negative_left {
        order.reverse()
    } else {
        order
    }))
}

pub(in crate::runtime::accounted::json_arena) fn compare(
    left: &Forest,
    left_root: NodeId,
    right: &Forest,
    right_root: NodeId,
    controls: &QueryExecutionControls,
) -> Result<Option<Ordering>, CassieError> {
    let left_empty = matches!(&left.nodes.get(left_root)?.kind, Kind::Array(a) if a.len() == 0);
    let right_empty = matches!(&right.nodes.get(right_root)?.kind, Kind::Array(a) if a.len() == 0);
    if left_empty || right_empty {
        return Ok(Some(right_empty.cmp(&left_empty)));
    }
    let mut stack = Stack::new(controls)?;
    stack.push(Task::Pair(left_root, right_root), controls)?;
    while let Some(task) = stack.tasks.pop() {
        controls.check_interruption()?;
        let order = match task {
            Task::Pair(a, b) => compare_pair(left, right, a, b, &mut stack, controls)?,
            Task::Array {
                left: a,
                right: b,
                index,
            } => compare_array(left, right, a, b, index, &mut stack, controls)?,
            task @ Task::Object { .. } => compare_object(left, right, task, &mut stack, controls)?,
        };
        let Some(order) = order else {
            return Ok(None);
        };
        if !order.is_eq() {
            return Ok(Some(order));
        }
    }
    Ok(Some(Ordering::Equal))
}

fn compare_pair(
    left: &Forest,
    right: &Forest,
    a: NodeId,
    b: NodeId,
    stack: &mut Stack,
    controls: &QueryExecutionControls,
) -> Result<Option<Ordering>, CassieError> {
    let a_kind = &left.nodes.get(a)?.kind;
    let b_kind = &right.nodes.get(b)?.kind;
    let class = rank(a_kind).cmp(&rank(b_kind));
    if !class.is_eq() {
        return Ok(Some(class));
    }
    let order = match (a_kind, b_kind) {
        (Kind::Scalar, Kind::Scalar) => Ordering::Equal,
        (Kind::Boolean(a), Kind::Boolean(b)) => a.cmp(b),
        (Kind::Text(a), Kind::Text(b)) => {
            bytes(a.as_str().as_bytes(), b.as_str().as_bytes(), controls)?
        }
        (Kind::Number(a), Kind::Number(b)) => {
            let Some(order) = number(a.as_str(), b.as_str(), controls)? else {
                return Ok(None);
            };
            order
        }
        (Kind::Array(a_index), Kind::Array(b_index)) => {
            let order = a_index.len().cmp(&b_index.len());
            if order.is_eq() {
                stack.push(
                    Task::Array {
                        left: a,
                        right: b,
                        index: 0,
                    },
                    controls,
                )?;
            }
            order
        }
        (Kind::Object(a_index), Kind::Object(b_index)) => {
            let order = a_index.values_len().cmp(&b_index.values_len());
            if order.is_eq() {
                let left_order = jsonb_order::sorted(a_index, controls, &mut || {})?;
                let right_order = jsonb_order::sorted(b_index, controls, &mut || {})?;
                stack.push(
                    Task::Object {
                        left: a,
                        right: b,
                        index: 0,
                        left_order,
                        right_order,
                    },
                    controls,
                )?;
            }
            order
        }
        _ => return Ok(None),
    };
    Ok(Some(order))
}

fn compare_array(
    left: &Forest,
    right: &Forest,
    a: NodeId,
    b: NodeId,
    index: usize,
    stack: &mut Stack,
    controls: &QueryExecutionControls,
) -> Result<Option<Ordering>, CassieError> {
    let (Kind::Array(a_index), Kind::Array(b_index)) =
        (&left.nodes.get(a)?.kind, &right.nodes.get(b)?.kind)
    else {
        return Ok(None);
    };
    if index < a_index.len() {
        let a_child = a_index.get(index).ok_or_else(super::invalid_owner)?;
        let b_child = b_index.get(index).ok_or_else(super::invalid_owner)?;
        stack.push(
            Task::Array {
                left: a,
                right: b,
                index: index + 1,
            },
            controls,
        )?;
        stack.push(Task::Pair(a_child, b_child), controls)?;
    }
    Ok(Some(Ordering::Equal))
}

fn compare_object(
    left: &Forest,
    right: &Forest,
    task: Task,
    stack: &mut Stack,
    controls: &QueryExecutionControls,
) -> Result<Option<Ordering>, CassieError> {
    let Task::Object {
        left: a,
        right: b,
        index,
        left_order,
        right_order,
    } = task
    else {
        unreachable!("object comparison task")
    };
    let (Kind::Object(a_index), Kind::Object(b_index)) =
        (&left.nodes.get(a)?.kind, &right.nodes.get(b)?.kind)
    else {
        return Ok(None);
    };
    if index < a_index.values_len() {
        let (a_key, a_child) =
            a_index.item(left_order.get(index).ok_or_else(super::invalid_owner)?);
        let (b_key, b_child) =
            b_index.item(right_order.get(index).ok_or_else(super::invalid_owner)?);
        let order = bytes(a_key.as_bytes(), b_key.as_bytes(), controls)?;
        if !order.is_eq() {
            return Ok(Some(order));
        }
        stack.push(
            Task::Object {
                left: a,
                right: b,
                index: index + 1,
                left_order,
                right_order,
            },
            controls,
        )?;
        stack.push(Task::Pair(a_child, b_child), controls)?;
    }
    Ok(Some(Ordering::Equal))
}
