//! Finite INNER/LEFT typed payload hash join with bounded match batches.
use std::collections::HashMap;
use std::mem::size_of;

use super::{check_controls, invalid, Column, QueryError, TypedBatch};
use crate::executor::retained_memory::{add, data_type_clone_bytes, hash_table_bytes, mul};
use crate::executor::semantic::SemanticValue;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::{DataType, Value};

pub(crate) struct HashJoin<'a> {
    left: &'a TypedBatch,
    right: &'a TypedBatch,
    left_key: usize,
    outer: bool,
    batch_size: usize,
    controls: QueryExecutionControls,
    build: HashMap<SemanticValue, (usize, usize)>,
    next: Vec<Option<usize>>,
    build_rows: usize,
    left_lane: usize,
    right_lane: Option<usize>,
    probing: bool,
    matched: bool,
    _memory: QueryMemoryReservation,
}

pub(crate) struct MatchBatch {
    pub(crate) batch: TypedBatch,
    pub(crate) left: Vec<usize>,
    pub(crate) right: Vec<Option<usize>>,
    _memory: QueryMemoryReservation,
}

impl<'a> HashJoin<'a> {
    pub(crate) fn new(
        controls: &QueryExecutionControls,
        left: &'a TypedBatch,
        right: &'a TypedBatch,
        keys: (usize, usize),
        outer: bool,
        batch_size: usize,
        build_probe: impl Fn(usize) -> Result<(), QueryError>,
    ) -> Result<Self, QueryError> {
        check_controls(controls)?;
        if !supports_keys(left, right, keys) {
            return Err(invalid("unsupported typed join key domains"));
        }
        let bytes = add(
            add(
                64,
                hash_table_bytes::<(SemanticValue, (usize, usize))>(right.len())?,
            )?,
            mul(right.len(), size_of::<Option<usize>>())?,
        )?;
        let memory = controls.reserve_query_memory(bytes)?;
        let mut build = HashMap::<SemanticValue, (usize, usize)>::new();
        build
            .try_reserve(right.len())
            .map_err(|error| allocation_error(&error))?;
        let mut next = Vec::new();
        next.try_reserve_exact(right.len())
            .map_err(|error| allocation_error(&error))?;
        next.resize(right.len(), None);
        let mut build_rows = 0;
        for lane in 0..right.len() {
            check_controls(controls)?;
            build_probe(lane)?;
            check_controls(controls)?;
            if let Some(key) = key(right.cell(keys.1, lane)?) {
                build_rows += 1;
                if let Some((_, tail)) = build.get_mut(&key) {
                    next[*tail] = Some(lane);
                    *tail = lane;
                } else {
                    build.insert(key, (lane, lane));
                }
            }
        }
        Ok(Self {
            left,
            right,
            left_key: keys.0,
            outer,
            batch_size: batch_size.max(1),
            controls: controls.clone(),
            build,
            next,
            build_rows,
            left_lane: 0,
            right_lane: None,
            probing: false,
            matched: false,
            _memory: memory,
        })
    }

    pub(crate) const fn build_rows(&self) -> usize {
        self.build_rows
    }

    pub(crate) fn next_batch(
        &mut self,
        remaining: usize,
    ) -> Result<Option<MatchBatch>, QueryError> {
        self.next_batch_with_probe(remaining, || {})
    }

    pub(crate) fn next_batch_with_probe(
        &mut self,
        remaining: usize,
        probe: impl Fn(),
    ) -> Result<Option<MatchBatch>, QueryError> {
        check_controls(&self.controls)?;
        if remaining == 0 || self.left_lane >= self.left.len() {
            return Ok(None);
        }
        let count = self.batch_size.min(remaining);
        let memory = self.controls.reserve_query_memory(mul(
            count,
            add(size_of::<usize>(), size_of::<Option<usize>>())?,
        )?)?;
        let mut left = Vec::new();
        let mut right = Vec::new();
        left.try_reserve_exact(count)
            .map_err(|error| allocation_error(&error))?;
        right
            .try_reserve_exact(count)
            .map_err(|error| allocation_error(&error))?;
        while left.len() < count && self.left_lane < self.left.len() {
            probe();
            check_controls(&self.controls)?;
            if !self.probing {
                self.right_lane = key(self.left.cell(self.left_key, self.left_lane)?)
                    .and_then(|key| self.build.get(&key).map(|(head, _)| *head));
                self.probing = true;
                self.matched = self.right_lane.is_some();
            }
            if let Some(lane) = self.right_lane {
                left.push(self.left_lane);
                right.push(Some(lane));
                self.right_lane = self.next[lane];
            } else {
                if self.outer && !self.matched {
                    left.push(self.left_lane);
                    right.push(None);
                }
                self.left_lane += 1;
                self.probing = false;
            }
        }
        if left.is_empty() {
            return Ok(None);
        }
        let batch = gather_output(&self.controls, self.left, self.right, &left, &right)?;
        check_controls(&self.controls)?;
        Ok(Some(MatchBatch {
            batch,
            left,
            right,
            _memory: memory,
        }))
    }
}

pub(crate) fn supports_keys(left: &TypedBatch, right: &TypedBatch, keys: (usize, usize)) -> bool {
    let domains = left.schema().get(keys.0).zip(right.schema().get(keys.1));
    domains.is_some_and(|((_, left), (_, right))| {
        super::capability::join_keys(left, right) == super::capability::Capability::NativeTyped
    })
}

fn key(cell: super::Cell<'_>) -> Option<SemanticValue> {
    let value = match cell {
        super::Cell::Integer(value) => Value::Int64(value),
        super::Cell::Float(value) => Value::Float64(value),
        super::Cell::Boolean(value) => Value::Bool(value),
        super::Cell::Scalar(value @ (Value::Int64(_) | Value::Float64(_) | Value::Bool(_))) => {
            value.clone()
        }
        _ => return None,
    };
    Some(SemanticValue::from_value(&value))
}

fn gather_output(
    controls: &QueryExecutionControls,
    left: &TypedBatch,
    right: &TypedBatch,
    left_lanes: &[usize],
    right_lanes: &[Option<usize>],
) -> Result<TypedBatch, QueryError> {
    let width = add(left.schema().len(), right.schema().len())?;
    let maps = mul(
        left_lanes.len(),
        add(2 * size_of::<usize>(), size_of::<bool>())?,
    )?;
    let mut bytes = add(
        maps,
        mul(
            width,
            add(size_of::<Column>(), size_of::<(String, DataType)>())?,
        )?,
    )?;
    for (name, data_type) in left.schema().iter().chain(right.schema()) {
        bytes = add(bytes, add(name.len(), data_type_clone_bytes(data_type)?)?)?;
    }
    let _memory = controls.reserve_query_memory(bytes)?;
    let mut left_positions = Vec::with_capacity(left_lanes.len());
    let mut right_positions = Vec::with_capacity(right_lanes.len());
    let mut validity = Vec::with_capacity(right_lanes.len());
    for lane in left_lanes {
        left_positions.push(left.position(*lane)?);
    }
    for lane in right_lanes {
        right_positions.push(lane.map_or(Ok(0), |lane| right.position(lane))?);
        validity.push(lane.is_some());
    }
    let mut columns = Vec::with_capacity(width);
    for column in left.columns.get() {
        check_controls(controls)?;
        columns.push(column.gather(controls, &left_positions)?);
    }
    for column in right.columns.get() {
        check_controls(controls)?;
        columns.push(if right.len() == 0 {
            Column::constant(controls, column.data_type(), &Value::Null, left_lanes.len())?
        } else {
            column.dictionary(controls, &right_positions, &validity)?
        });
    }
    let schema = left
        .schema()
        .iter()
        .chain(right.schema())
        .cloned()
        .collect::<Vec<_>>();
    TypedBatch::from_views(controls, &schema, &columns, left_lanes.len(), None)
}

fn allocation_error(error: &std::collections::TryReserveError) -> QueryError {
    crate::app::CassieError::ResourceLimit(format!("unable to retain typed hash join: {error}"))
        .into()
}

#[cfg(test)]
#[path = "join/tests.rs"]
mod tests;
