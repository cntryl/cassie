//! Scoped, borrowing observations of the actual DML result helper.
use super::super::{BatchRow, QueryExecutionControls};
use crate::executor::batch::OperatorMemory;
use crate::runtime::QueryMemoryReservation;
use crate::types::Value;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Weak};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::executor::execution::dml) enum Phase {
    Entry,
    Projected,
    Materializing(usize),
}

pub(super) struct Observation<'a> {
    pub phase: Phase,
    pub controls: &'a QueryExecutionControls,
    pub affected: usize,
    pub rows: &'a [BatchRow],
    pub capacity: usize,
}

type Callback = Box<dyn FnMut(Observation<'_>)>;
thread_local! {
    static OBSERVER: RefCell<Option<Callback>> = RefCell::new(None);
}

pub(in crate::executor::execution::dml) fn observe(
    phase: Phase,
    controls: &QueryExecutionControls,
    affected: usize,
    rows: &[BatchRow],
    capacity: usize,
) {
    let callback = OBSERVER.with(|slot| slot.borrow_mut().take());
    if let Some(mut callback) = callback {
        callback(Observation {
            phase,
            controls,
            affected,
            rows,
            capacity,
        });
        OBSERVER.with(|slot| {
            assert!(
                slot.borrow().is_none(),
                "callback cannot replace its scoped observer"
            );
            *slot.borrow_mut() = Some(callback);
        });
    }
}

pub(super) struct Hook;
impl Hook {
    pub fn install(callback: impl FnMut(Observation<'_>) + 'static) -> Self {
        OBSERVER.with(|slot| {
            assert!(slot.borrow().is_none(), "one DML observer per thread");
            *slot.borrow_mut() = Some(Box::new(callback));
        });
        Self
    }
}
impl Drop for Hook {
    fn drop(&mut self) {
        OBSERVER.with(|slot| slot.borrow_mut().take());
    }
}

#[derive(Debug)]
pub(super) struct Roots {
    pub current: Option<Weak<QueryMemoryReservation>>,
    pub operator: Option<Weak<OperatorMemory>>,
    pub ancestors: Vec<Weak<QueryMemoryReservation>>,
}
impl Roots {
    fn of(row: &BatchRow) -> Self {
        let mut ancestors = Vec::new();
        let mut outer = row.outer_scope();
        while let Some(row) = outer {
            if let Some(memory) = row.query_memory() {
                ancestors.push(Arc::downgrade(&memory));
            }
            outer = row.outer_scope();
        }
        Self {
            current: row.query_memory().as_ref().map(Arc::downgrade),
            operator: row.operator_memory().as_ref().map(Arc::downgrade),
            ancestors,
        }
    }

    pub fn alive(&self) -> bool {
        self.current
            .as_ref()
            .is_none_or(|root| root.strong_count() > 0)
            && self
                .operator
                .as_ref()
                .is_none_or(|root| root.strong_count() > 0)
            && self.ancestors.iter().all(|root| root.strong_count() > 0)
    }

    pub fn retired(&self) -> bool {
        self.current
            .as_ref()
            .is_none_or(|root| root.strong_count() == 0)
            && self
                .operator
                .as_ref()
                .is_none_or(|root| root.strong_count() == 0)
            && self.ancestors.iter().all(|root| root.strong_count() == 0)
    }
}

#[derive(Debug)]
pub(super) struct TextBuffer {
    pub id: i64,
    pub name: String,
    pub address: usize,
    pub length: usize,
    pub capacity: usize,
}

fn row_id(row: &BatchRow) -> i64 {
    match row
        .entries()
        .iter()
        .find(|(name, _)| name == "id")
        .map(|(_, value)| value)
    {
        Some(Value::Int64(id)) => *id,
        other => panic!("actual BIGINT RETURNING row: {other:?}"),
    }
}

pub(super) struct Record {
    pub phase: Phase,
    pub affected: usize,
    pub ids: Vec<i64>,
    pub current: usize,
    pub peak: usize,
    pub backing: usize,
    pub text: Vec<TextBuffer>,
    pub roots: Vec<Roots>,
    pub entry_roots_alive: bool,
    pub preceding_roots_alive: bool,
}

impl std::fmt::Debug for Record {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Record")
            .field("phase", &self.phase)
            .field("affected", &self.affected)
            .field("ids", &self.ids)
            .field("current", &self.current)
            .field("peak", &self.peak)
            .field("backing", &self.backing)
            .field("text_buffers", &self.text.len())
            .field("root_rows", &self.roots.len())
            .field("entry_roots_alive", &self.entry_roots_alive)
            .field("preceding_roots_alive", &self.preceding_roots_alive)
            .finish_non_exhaustive()
    }
}

pub(super) type Records = Rc<RefCell<Vec<Record>>>;

pub(super) fn record(records: &Records, observation: &Observation<'_>) {
    let mut records = records.borrow_mut();
    let entry_roots_alive = records
        .iter()
        .find(|record| record.phase == Phase::Entry)
        .is_none_or(|entry| entry.roots.iter().all(Roots::alive));
    let preceding_roots_alive = match observation.phase {
        Phase::Materializing(index) => records
            .iter()
            .find(|record| record.phase == Phase::Projected)
            .is_some_and(|projected| projected.roots[..index].iter().all(Roots::alive)),
        _ => true,
    };
    let backing = observation.capacity * std::mem::size_of::<BatchRow>()
        + observation
            .rows
            .iter()
            .map(|row| row.owned_body_bytes().expect("actual row buffers"))
            .sum::<usize>();
    let mut ids = observation.rows.iter().map(row_id).collect::<Vec<_>>();
    ids.sort_unstable();
    let text = observation
        .rows
        .iter()
        .flat_map(|row| {
            row.entries().iter().filter_map(|(name, value)| {
                let Value::String(value) = value else {
                    return None;
                };
                Some(TextBuffer {
                    id: row_id(row),
                    name: name.clone(),
                    address: value.as_ptr() as usize,
                    length: value.len(),
                    capacity: value.capacity(),
                })
            })
        })
        .collect();
    records.push(Record {
        phase: observation.phase,
        affected: observation.affected,
        ids,
        current: observation.controls.current_query_memory_bytes(),
        peak: observation.controls.peak_query_memory_bytes(),
        backing,
        text,
        roots: observation.rows.iter().map(Roots::of).collect(),
        entry_roots_alive,
        preceding_roots_alive,
    });
}
