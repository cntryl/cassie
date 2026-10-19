//! `sum(interval)` and `avg(interval)` accumulation.
use super::state::{AggregateAccumulator, RetainedChange};
use super::QueryError;
use crate::types::extended::temporal::Interval;
use crate::types::typed::TypedValue;
use crate::types::Value;

/// An interval sum with its input count, for `sum` and `avg`.
#[derive(Clone, Copy, Debug)]
pub(in crate::executor::execution) struct IntervalAggregate {
    sum: Interval,
    count: i64,
    average: bool,
}

fn overflow() -> QueryError {
    QueryError::Cassie(crate::app::CassieError::TypeInput(
        crate::types::extended::temporal::range_error("interval out of range"),
    ))
}

impl IntervalAggregate {
    fn add(&mut self, value: &Interval) -> Result<(), QueryError> {
        self.sum = self.sum.add(value).map_err(|_| overflow())?;
        self.count += 1;
        Ok(())
    }

    pub(super) fn merge(&mut self, other: &Self) -> Result<(), QueryError> {
        self.sum = self.sum.add(&other.sum).map_err(|_| overflow())?;
        self.count += other.count;
        Ok(())
    }

    pub(super) fn finish(self) -> Result<Value, QueryError> {
        if self.count == 0 {
            return Ok(Value::Null);
        }
        if !self.average {
            return Ok(Value::Typed(TypedValue::Interval(self.sum)));
        }
        let count = num_traits::ToPrimitive::to_f64(&self.count).unwrap_or(1.0);
        self.sum
            .div(count)
            .map(|value| Value::Typed(TypedValue::Interval(value)))
            .map_err(|_| overflow())
    }
}

/// Folds an interval input; `None` leaves other inputs to the numeric path.
pub(super) fn update(
    accumulator: &mut AggregateAccumulator,
    value: &Value,
) -> Option<Result<RetainedChange, QueryError>> {
    if let AggregateAccumulator::Interval(state) = accumulator {
        return Some(match value {
            Value::Null => Ok(RetainedChange::default()),
            Value::Typed(TypedValue::Interval(interval)) => {
                state.add(interval).map(|()| RetainedChange::default())
            }
            _ => Err(QueryError::General(
                "function sum(interval) cannot mix input types".into(),
            )),
        });
    }
    let Value::Typed(TypedValue::Interval(interval)) = value else {
        return None;
    };
    let average = match accumulator {
        AggregateAccumulator::Sum { seen: false, .. } => false,
        AggregateAccumulator::Avg { count: 0, .. } => true,
        _ => return None,
    };
    let mut state = IntervalAggregate {
        sum: Interval::default(),
        count: 0,
        average,
    };
    if let Err(error) = state.add(interval) {
        return Some(Err(error));
    }
    *accumulator = AggregateAccumulator::Interval(state);
    Some(Ok(RetainedChange::default()))
}
