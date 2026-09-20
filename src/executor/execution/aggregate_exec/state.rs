use crate::types::numeric::{i64_to_f64, usize_to_f64};
use std::collections::HashMap;

use crate::app::CassieSession;
use crate::catalog::FunctionMeta;
use crate::executor::batch::BatchRow;
use crate::executor::filter;
use crate::executor::semantic::compare_values;
use crate::sql::ast::{Expr, FunctionCall};
use crate::types::Value;

use super::{AggregateExecutionContext, AggregateSpec, QueryError};

struct AggregateValueContext<'a> {
    params: &'a [Value],
    search_context: Option<&'a filter::SearchContext>,
    user_functions: &'a HashMap<String, FunctionMeta>,
    session: Option<&'a CassieSession>,
}

#[derive(Clone)]
pub(super) struct PartialAggregateGroup {
    pub(super) group_values: Vec<(String, Value)>,
    pub(super) accumulators: Vec<AggregateAccumulator>,
}

impl PartialAggregateGroup {
    pub(super) fn new(group_values: Vec<(String, Value)>, specs: &[AggregateSpec]) -> Self {
        Self {
            group_values,
            accumulators: specs
                .iter()
                .map(|spec| AggregateAccumulator::new(&spec.function))
                .collect(),
        }
    }

    /// Folds `row` into the accumulators and reports how their retained
    /// bytes changed.
    pub(super) fn update(
        &mut self,
        row: &BatchRow,
        specs: &[AggregateSpec],
        context: &AggregateExecutionContext<'_>,
    ) -> Result<RetainedChange, QueryError> {
        let mut change = RetainedChange::default();
        for (accumulator, spec) in self.accumulators.iter_mut().zip(specs) {
            change.absorb(accumulator.update(
                &spec.function,
                row,
                context.params,
                context.search_context,
                context.user_functions,
                context.session,
            )?);
        }
        Ok(change)
    }

    /// Merges `other` into this group and reports how the retained
    /// accumulator bytes changed.
    pub(super) fn merge(&mut self, other: &Self) -> Result<RetainedChange, QueryError> {
        let mut change = RetainedChange::default();
        for (left, right) in self.accumulators.iter_mut().zip(&other.accumulators) {
            change.absorb(left.merge(right)?);
        }
        Ok(change)
    }
}

/// Retained accumulator bytes before and after an update or merge.
#[derive(Clone, Copy, Default)]
pub(super) struct RetainedChange {
    pub(super) before: usize,
    pub(super) after: usize,
}

impl RetainedChange {
    fn replaced(before: Option<&Value>, after: &Value) -> Self {
        Self {
            before: before.map_or(0, value_retained_bytes),
            after: value_retained_bytes(after),
        }
    }

    fn absorb(&mut self, other: Self) {
        self.before = self.before.saturating_add(other.before);
        self.after = self.after.saturating_add(other.after);
    }
}

fn value_retained_bytes(value: &Value) -> usize {
    super::group_memory::json_bytes(value)
}

#[derive(Clone)]
pub(super) enum AggregateAccumulator {
    Count { count: i64 },
    Sum { sum: NumericSum, seen: bool },
    Avg { sum: f64, count: usize },
    MinMax { selected: Option<Value>, max: bool },
}

impl AggregateAccumulator {
    /// Accounted bytes for this accumulator, including a retained MIN/MAX
    /// value whose size depends on the input (for example long text).
    pub(super) fn retained_bytes(&self) -> usize {
        let inline = std::mem::size_of::<Self>();
        match self {
            Self::MinMax {
                selected: Some(value),
                ..
            } => inline.saturating_add(value_retained_bytes(value)),
            _ => inline,
        }
    }

    fn new(function: &FunctionCall) -> Self {
        match function.name.to_ascii_lowercase().as_str() {
            "count" => Self::Count { count: 0 },
            "sum" => Self::Sum {
                sum: NumericSum::Int(0),
                seen: false,
            },
            "avg" => Self::Avg { sum: 0.0, count: 0 },
            "max" => Self::MinMax {
                selected: None,
                max: true,
            },
            _ => Self::MinMax {
                selected: None,
                max: false,
            },
        }
    }

    fn update(
        &mut self,
        function: &FunctionCall,
        row: &BatchRow,
        params: &[Value],
        search_context: Option<&filter::SearchContext>,
        user_functions: &HashMap<String, FunctionMeta>,
        session: Option<&CassieSession>,
    ) -> Result<RetainedChange, QueryError> {
        let value_context = AggregateValueContext {
            params,
            search_context,
            user_functions,
            session,
        };
        match self {
            Self::Count { count } => Self::update_count(function, row, &value_context, count)?,
            Self::Sum { sum, seen } => {
                Self::update_sum(function, row, &value_context, sum, seen)?;
            }
            Self::Avg { sum, count } => {
                Self::update_avg(function, row, &value_context, sum, count)?;
            }
            Self::MinMax { selected, max } => {
                return Self::update_minmax(function, row, &value_context, selected, *max);
            }
        }
        Ok(RetainedChange::default())
    }

    fn merge(&mut self, other: &Self) -> Result<RetainedChange, QueryError> {
        match (self, other) {
            (Self::Count { count }, Self::Count { count: other }) => {
                *count = count
                    .checked_add(*other)
                    .ok_or_else(|| QueryError::General("aggregate count overflow".to_string()))?;
            }
            (
                Self::Sum { sum, seen },
                Self::Sum {
                    sum: other_sum,
                    seen: other_seen,
                },
            ) => {
                sum.merge(other_sum)?;
                *seen = *seen || *other_seen;
            }
            (
                Self::Avg { sum, count },
                Self::Avg {
                    sum: other_sum,
                    count: other_count,
                },
            ) => {
                *sum += other_sum;
                *count += other_count;
            }
            (
                Self::MinMax { selected, max },
                Self::MinMax {
                    selected: Some(value),
                    max: _,
                },
            ) => {
                let replace = selected.as_ref().is_none_or(|current| {
                    let ordering = compare_values(value, current);
                    if *max {
                        ordering.is_gt()
                    } else {
                        ordering.is_lt()
                    }
                });
                if replace {
                    let change = RetainedChange::replaced(selected.as_ref(), value);
                    *selected = Some(value.clone());
                    return Ok(change);
                }
            }
            _ => {}
        }
        Ok(RetainedChange::default())
    }

    pub(super) fn finish(self) -> Value {
        match self {
            Self::Count { count } => Value::Int64(count),
            Self::Sum { sum, seen } => {
                if seen {
                    sum.finish_value()
                } else {
                    Value::Null
                }
            }
            Self::Avg { sum, count } => {
                if count == 0 {
                    Value::Null
                } else {
                    let count = usize_to_f64(count);
                    Value::Float64(sum / count)
                }
            }
            Self::MinMax { selected, .. } => selected.unwrap_or(Value::Null),
        }
    }

    fn evaluate_input(
        function: &FunctionCall,
        row: &BatchRow,
        context: &AggregateValueContext<'_>,
    ) -> Result<Option<Value>, QueryError> {
        let Some(expr) = function.args.first() else {
            return Ok(None);
        };
        filter::evaluate_expr_value(
            row,
            expr,
            context.params,
            context.search_context,
            context.user_functions,
            context.session,
            None,
        )
        .map(Some)
    }

    fn update_count(
        function: &FunctionCall,
        row: &BatchRow,
        context: &AggregateValueContext<'_>,
        count: &mut i64,
    ) -> Result<(), QueryError> {
        if matches!(function.args.as_slice(), [Expr::Column(name)] if name == "*") {
            *count += 1;
            return Ok(());
        }
        let Some(value) = Self::evaluate_input(function, row, context)? else {
            return Ok(());
        };
        if !matches!(value, Value::Null) {
            *count += 1;
        }
        Ok(())
    }

    fn update_sum(
        function: &FunctionCall,
        row: &BatchRow,
        context: &AggregateValueContext<'_>,
        sum: &mut NumericSum,
        seen: &mut bool,
    ) -> Result<(), QueryError> {
        let Some(value) = Self::evaluate_input(function, row, context)? else {
            return Ok(());
        };
        match value {
            Value::Int64(value) => {
                sum.add_int(value)?;
                *seen = true;
            }
            Value::Float64(value) => {
                sum.add_float(value);
                *seen = true;
            }
            Value::Null => {}
            _ => sum.promote_to_float(),
        }
        Ok(())
    }

    fn update_avg(
        function: &FunctionCall,
        row: &BatchRow,
        context: &AggregateValueContext<'_>,
        sum: &mut f64,
        count: &mut usize,
    ) -> Result<(), QueryError> {
        let Some(value) = Self::evaluate_input(function, row, context)? else {
            return Ok(());
        };
        match value {
            Value::Int64(value) => {
                *sum += i64_to_f64(value);
                *count += 1;
            }
            Value::Float64(value) => {
                *sum += value;
                *count += 1;
            }
            _ => {}
        }
        Ok(())
    }

    fn update_minmax(
        function: &FunctionCall,
        row: &BatchRow,
        context: &AggregateValueContext<'_>,
        selected: &mut Option<Value>,
        max: bool,
    ) -> Result<RetainedChange, QueryError> {
        let Some(value) = Self::evaluate_input(function, row, context)? else {
            return Ok(RetainedChange::default());
        };
        if matches!(value, Value::Null) {
            return Ok(RetainedChange::default());
        }
        let replace = selected.as_ref().is_none_or(|current| {
            let ordering = compare_values(&value, current);
            if max {
                ordering.is_gt()
            } else {
                ordering.is_lt()
            }
        });
        if replace {
            let change = RetainedChange::replaced(selected.as_ref(), &value);
            *selected = Some(value);
            return Ok(change);
        }
        Ok(RetainedChange::default())
    }
}

#[derive(Clone)]
pub(super) enum NumericSum {
    Int(i64),
    Float(f64),
}

impl NumericSum {
    pub(super) fn add_int(&mut self, value: i64) -> Result<(), QueryError> {
        match self {
            Self::Int(sum) => {
                *sum = sum.checked_add(value).ok_or_else(|| {
                    QueryError::General(String::from("aggregate integer overflow"))
                })?;
            }
            Self::Float(sum) => *sum += i64_to_f64(value),
        }
        Ok(())
    }

    pub(super) fn add_float(&mut self, value: f64) {
        self.promote_to_float();
        if let Self::Float(sum) = self {
            *sum += value;
        }
    }

    pub(super) fn promote_to_float(&mut self) {
        if let Self::Int(sum) = self {
            *self = Self::Float(i64_to_f64(*sum));
        }
    }

    fn merge(&mut self, other: &Self) -> Result<(), QueryError> {
        match other {
            Self::Int(value) => self.add_int(*value),
            Self::Float(value) => {
                self.add_float(*value);
                Ok(())
            }
        }
    }

    pub(super) fn finish_value(self) -> Value {
        match self {
            Self::Int(sum) => Value::Int64(sum),
            Self::Float(sum) => Value::Float64(sum),
        }
    }
}
