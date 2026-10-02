use crate::types::numeric::{self, i128_to_f64, i64_to_f64, usize_to_f64};
use std::collections::HashMap;

use crate::app::CassieSession;
use crate::catalog::FunctionMeta;
use crate::executor::batch::BatchRow;
use crate::executor::filter;
use crate::executor::semantic::{compare_values, SemanticValue};
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

    /// True when any accumulator must be folded in row order rather than
    /// merged from partitions.
    pub(super) fn requires_row_order(&self) -> bool {
        self.accumulators
            .iter()
            .any(AggregateAccumulator::requires_row_order)
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
    fn replaced(
        before: Option<&Value>,
        after: &Value,
        old_key: Option<&SemanticValue>,
        new_key: Option<&SemanticValue>,
    ) -> Self {
        Self {
            before: before
                .map_or(0, value_retained_bytes)
                .saturating_add(old_key.map_or(0, SemanticValue::estimated_bytes)),
            after: value_retained_bytes(after)
                .saturating_add(new_key.map_or(0, SemanticValue::estimated_bytes)),
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
    Count {
        count: i64,
    },
    Sum {
        sum: NumericSum,
        seen: bool,
    },
    Avg {
        sum: AvgSum,
        count: usize,
    },
    MinMax {
        selected: Option<Value>,
        key: Option<SemanticValue>,
        max: bool,
    },
}

impl AggregateAccumulator {
    /// Accounted bytes for this accumulator, including a retained MIN/MAX
    /// value whose size depends on the input (for example long text).
    pub(super) fn retained_bytes(&self) -> usize {
        let inline = std::mem::size_of::<Self>();
        match self {
            Self::MinMax {
                selected: Some(value),
                key,
                ..
            } => inline
                .saturating_add(value_retained_bytes(value))
                .saturating_add(key.as_ref().map_or(0, SemanticValue::estimated_bytes)),
            _ => inline,
        }
    }

    /// Folds `rows` in order through a fresh accumulator for `function`.
    pub(super) fn evaluate(
        function: &FunctionCall,
        rows: &[BatchRow],
        context: &AggregateExecutionContext<'_>,
    ) -> Result<Value, QueryError> {
        let mut accumulator = Self::new(function);
        let mut key_memory = super::group_memory::GroupMemory::new(context.controls)?;
        let mut retained_key_bytes = 0;
        for row in rows {
            accumulator.update(
                function,
                row,
                context.params,
                context.search_context,
                context.user_functions,
                context.session,
            )?;
            let bytes = match &accumulator {
                Self::MinMax { key: Some(key), .. } => key.estimated_bytes(),
                _ => 0,
            };
            key_memory.resize(retained_key_bytes, bytes)?;
            retained_key_bytes = bytes;
        }
        accumulator.finish()
    }

    fn new(function: &FunctionCall) -> Self {
        match function.name.to_ascii_lowercase().as_str() {
            "count" => Self::Count { count: 0 },
            "sum" => Self::Sum {
                sum: NumericSum::default(),
                seen: false,
            },
            "avg" => Self::Avg {
                sum: AvgSum::default(),
                count: 0,
            },
            "max" => Self::MinMax {
                selected: None,
                key: None,
                max: true,
            },
            _ => Self::MinMax {
                selected: None,
                key: None,
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
            Self::MinMax { selected, key, max } => {
                return Self::update_minmax(function, row, &value_context, selected, key, *max);
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
                sum.merge(other_sum);
                *seen = *seen || *other_seen;
            }
            (
                Self::Avg { sum, count },
                Self::Avg {
                    sum: other_sum,
                    count: other_count,
                },
            ) => {
                sum.merge(other_sum);
                *count += other_count;
            }
            (
                Self::MinMax { selected, key, max },
                Self::MinMax {
                    selected: Some(value),
                    key: other_key,
                    max: _,
                },
            ) => {
                let replace = selected.as_ref().is_none_or(|current| {
                    let ordering = key.as_ref().zip(other_key.as_ref()).map_or_else(
                        || compare_values(value, current),
                        |(current, other)| other.cmp(current),
                    );
                    if *max {
                        ordering.is_gt()
                    } else {
                        ordering.is_lt()
                    }
                });
                if replace {
                    let change = RetainedChange::replaced(
                        selected.as_ref(),
                        value,
                        key.as_ref(),
                        other_key.as_ref(),
                    );
                    *selected = Some(value.clone());
                    key.clone_from(other_key);
                    return Ok(change);
                }
            }
            _ => {}
        }
        Ok(RetainedChange::default())
    }

    pub(super) fn finish(self) -> Result<Value, QueryError> {
        Ok(match self {
            Self::Count { count } => Value::Int64(count),
            Self::Sum { sum, seen } => {
                if seen {
                    sum.finish_value()?
                } else {
                    Value::Null
                }
            }
            Self::Avg { sum, count } => {
                if count == 0 {
                    Value::Null
                } else {
                    Value::Float64(sum.finish_mean(count)?)
                }
            }
            Self::MinMax { selected, .. } => selected.unwrap_or(Value::Null),
        })
    }

    /// True when this accumulator folded a float, so partitions of it cannot
    /// be merged without re-associating the row-order sum.
    pub(super) fn requires_row_order(&self) -> bool {
        match self {
            Self::Sum { sum, .. } => sum.requires_row_order(),
            Self::Avg { sum, .. } => sum.requires_row_order(),
            Self::Count { .. } | Self::MinMax { .. } => false,
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
                sum.add_int(value);
                *seen = true;
            }
            Value::Float64(value) => {
                sum.add_float(value);
                *seen = true;
            }
            Value::Null => {}
            other => return Err(non_numeric_input(function, &other)),
        }
        Ok(())
    }

    fn update_avg(
        function: &FunctionCall,
        row: &BatchRow,
        context: &AggregateValueContext<'_>,
        sum: &mut AvgSum,
        count: &mut usize,
    ) -> Result<(), QueryError> {
        let Some(value) = Self::evaluate_input(function, row, context)? else {
            return Ok(());
        };
        match value {
            Value::Int64(value) => {
                sum.add_int(value);
                *count += 1;
            }
            Value::Float64(value) => {
                sum.add_float(value);
                *count += 1;
            }
            Value::Null => {}
            other => return Err(non_numeric_input(function, &other)),
        }
        Ok(())
    }

    fn update_minmax(
        function: &FunctionCall,
        row: &BatchRow,
        context: &AggregateValueContext<'_>,
        selected: &mut Option<Value>,
        key: &mut Option<SemanticValue>,
        max: bool,
    ) -> Result<RetainedChange, QueryError> {
        let stored = function
            .args
            .first()
            .and_then(|expr| stored_json_column_value(row, expr));
        let Some(value) = (match stored {
            Some(value) => Some(value),
            None => Self::evaluate_input(function, row, context)?,
        }) else {
            return Ok(RetainedChange::default());
        };
        if matches!(value, Value::Null) {
            return Ok(RetainedChange::default());
        }
        let new_key = function
            .args
            .first()
            .map(|expr| {
                crate::executor::array_order::key(row, expr, &value, context.user_functions)
            })
            .filter(|key| matches!(key, SemanticValue::Array(_)));
        let replace = selected.as_ref().is_none_or(|current| {
            let ordering = key.as_ref().zip(new_key.as_ref()).map_or_else(
                || compare_values(&value, current),
                |(current, new)| new.cmp(current),
            );
            if max {
                ordering.is_gt()
            } else {
                ordering.is_lt()
            }
        });
        if replace {
            let change =
                RetainedChange::replaced(selected.as_ref(), &value, key.as_ref(), new_key.as_ref());
            *selected = Some(value);
            *key = new_key;
            return Ok(change);
        }
        Ok(RetainedChange::default())
    }
}

/// The stored value of a bare column reference that holds a JSON value (an
/// array or json document), read directly from the row. Scalar expression
/// evaluation flattens JSON to text, which would turn an array column's
/// group key or MIN/MAX result into a string under an array-typed column.
pub(super) fn stored_json_column_value(row: &BatchRow, expr: &Expr) -> Option<Value> {
    let Expr::Column(name) = expr else {
        return None;
    };
    match row.get(name) {
        Some(value @ Value::Json(_)) => Some(value.clone()),
        _ => None,
    }
}

/// Running SUM state that reproduces the sequential row-order fold.
///
/// Integer inputs accumulate exactly while tracking the running prefix
/// range, so a merge of ordered partitions reports overflow exactly when the
/// row-order running total would have left `i64`. Once a non-integer input
/// arrives the state becomes a row-order `f64` fold, which cannot be
/// re-associated; see [`NumericSum::requires_row_order`].
#[derive(Clone)]
pub(super) enum NumericSum {
    Int(IntegerSum),
    Float {
        sum: f64,
        overflow: Option<SumOverflow>,
    },
}

/// The first overflow a row-order fold hit. It is reported when the fold
/// finishes, so an ordered partition that only overflows locally never
/// fails a query whose full row order stays in range.
#[derive(Clone, Copy)]
pub(super) enum SumOverflow {
    Integer,
    Float,
}

impl SumOverflow {
    fn error(self) -> QueryError {
        match self {
            Self::Integer => integer_overflow(),
            Self::Float => QueryError::General(String::from(numeric::FLOAT_OVERFLOW)),
        }
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct IntegerSum {
    total: i128,
    prefix_min: i128,
    prefix_max: i128,
}

impl IntegerSum {
    fn add(&mut self, value: i64) {
        self.total += i128::from(value);
        self.prefix_min = self.prefix_min.min(self.total);
        self.prefix_max = self.prefix_max.max(self.total);
    }

    fn append(&mut self, later: &Self) {
        self.prefix_min = self.prefix_min.min(self.total + later.prefix_min);
        self.prefix_max = self.prefix_max.max(self.total + later.prefix_max);
        self.total += later.total;
    }

    fn overflowed(&self) -> bool {
        self.prefix_min < i128::from(i64::MIN) || self.prefix_max > i128::from(i64::MAX)
    }
}

impl Default for NumericSum {
    fn default() -> Self {
        Self::Int(IntegerSum::default())
    }
}

impl NumericSum {
    pub(super) fn add_int(&mut self, value: i64) {
        match self {
            Self::Int(sum) => sum.add(value),
            Self::Float { .. } => self.add_float(i64_to_f64(value)),
        }
    }

    pub(super) fn add_float(&mut self, value: f64) {
        self.promote_to_float();
        if let Self::Float { sum, overflow } = self {
            if numeric::add_f64_overflowed(sum, value) && overflow.is_none() {
                *overflow = Some(SumOverflow::Float);
            }
        }
    }

    pub(super) fn promote_to_float(&mut self) {
        if let Self::Int(sum) = self {
            *self = Self::Float {
                sum: i128_to_f64(sum.total),
                overflow: sum.overflowed().then_some(SumOverflow::Integer),
            };
        }
    }

    /// True once a float participated: the state is then a row-order `f64`
    /// fold that a partition merge cannot reproduce.
    pub(super) fn requires_row_order(&self) -> bool {
        matches!(self, Self::Float { .. })
    }

    /// Appends the state of the rows that follow this one. Exact for
    /// integer states; float states must be folded in row order instead.
    fn merge(&mut self, later: &Self) {
        match (&mut *self, later) {
            (Self::Int(sum), Self::Int(later)) => sum.append(later),
            (_, Self::Int(later)) => self.add_float(i128_to_f64(later.total)),
            (_, Self::Float { sum, .. }) => self.add_float(*sum),
        }
    }

    pub(super) fn finish_value(self) -> Result<Value, QueryError> {
        match self {
            Self::Int(sum) => i64::try_from(sum.total)
                .ok()
                .filter(|_| !sum.overflowed())
                .map(Value::Int64)
                .ok_or_else(integer_overflow),
            Self::Float {
                overflow: Some(overflow),
                ..
            } => Err(overflow.error()),
            Self::Float { sum, .. } => Ok(Value::Float64(sum)),
        }
    }
}

/// Running AVG state: the row-order `f64` fold of every input, plus the
/// exact integer prefix range that shows when partition sums can be merged
/// without changing that fold.
#[derive(Clone, Default)]
pub(super) struct AvgSum {
    sum: f64,
    exact: IntegerSum,
    saw_float: bool,
    overflowed: bool,
}

impl AvgSum {
    /// Integers up to 2^53 are exact in `f64`; bounding every prefix by
    /// 2^52 also bounds every input by 2^53, so each addition is exact and
    /// any grouping of the fold yields the same value.
    const EXACT_PREFIX: i128 = 1 << 52;

    fn add_int(&mut self, value: i64) {
        self.overflowed |= numeric::add_f64_overflowed(&mut self.sum, i64_to_f64(value));
        self.exact.add(value);
    }

    fn add_float(&mut self, value: f64) {
        self.overflowed |= numeric::add_f64_overflowed(&mut self.sum, value);
        self.saw_float = true;
    }

    fn merge(&mut self, later: &Self) {
        self.overflowed |=
            numeric::add_f64_overflowed(&mut self.sum, later.sum) || later.overflowed;
        self.exact.append(&later.exact);
        self.saw_float |= later.saw_float;
    }

    fn finish_mean(&self, count: usize) -> Result<f64, QueryError> {
        if self.overflowed {
            return Err(SumOverflow::Float.error());
        }
        Ok(self.sum / usize_to_f64(count))
    }

    /// True when the fold is not exact, so merged partition sums could
    /// differ from the row-order fold.
    fn requires_row_order(&self) -> bool {
        self.saw_float
            || self.exact.prefix_min < -Self::EXACT_PREFIX
            || self.exact.prefix_max > Self::EXACT_PREFIX
    }
}

fn non_numeric_input(function: &FunctionCall, value: &Value) -> QueryError {
    QueryError::General(numeric::non_numeric_aggregate_input(&function.name, value))
}

fn integer_overflow() -> QueryError {
    QueryError::General(String::from("aggregate integer overflow"))
}
