use crate::types::numeric::{self, i128_to_f64, i64_to_f64, usize_to_f64};
use std::collections::HashMap;

use crate::app::{CassieError, CassieSession};
use crate::catalog::FunctionMeta;
use crate::executor::batch::BatchRow;
use crate::executor::filter;
use crate::executor::semantic::{compare_values, SemanticValue};
use crate::sql::ast::FunctionCall;
use crate::types::extended::numeric::{math as numeric_math, Numeric};
use crate::types::typed::TypedValue;
use crate::types::Value;

use super::bitwise::BitwiseState;
use super::boolean::BooleanState;
use super::regression::RegressionAccumulator;
use super::retained_change::{value_retained_bytes, RetainedChange};
use super::state_input::{self, AggregateValueContext};
use super::statistical::StatisticalAccumulator;
use super::{AggregateExecutionContext, AggregateSpec, QueryError};

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

#[derive(Clone)]
pub(super) enum AggregateAccumulator {
    Boolean(BooleanState),
    Bitwise(BitwiseState),
    Count {
        count: i64,
    },
    StringAgg {
        value: Option<String>,
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
    /// `sum`/`avg` over `interval` inputs.
    Interval(super::interval_aggregate::IntervalAggregate),
    Regression(RegressionAccumulator),
    Statistical(StatisticalAccumulator),
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
            Self::StringAgg { value: Some(value) } => inline.saturating_add(value.len()),
            Self::Regression(_) => {
                inline.saturating_add(RegressionAccumulator::retained_heap_bytes())
            }
            Self::Statistical(state) => inline.saturating_add(state.retained_heap_bytes()),
            _ => inline,
        }
    }

    /// Folds `rows` in order through a fresh accumulator for `function`.
    pub(super) fn evaluate(
        function: &FunctionCall,
        rows: &[BatchRow],
        context: &AggregateExecutionContext<'_>,
    ) -> Result<Value, QueryError> {
        let is_regression =
            crate::sql::functions::function(&function.name).is_some_and(|metadata| {
                crate::sql::functions::regression::is_regression_function(metadata.id)
            });
        if is_regression && (function.is_distinct() || !function.order_by.is_empty()) {
            return Err(CassieError::Unsupported(
                "DISTINCT and aggregate-local ORDER BY are not supported for regression aggregates"
                    .into(),
            )
            .into());
        }
        if is_string_aggregate(function) || function.is_distinct() || !function.order_by.is_empty()
        {
            return super::modifiers::evaluate(function, rows, context);
        }
        let mut accumulator = Self::new(function);
        let mut key_memory = super::group_memory::GroupMemory::new(context.controls)?;
        let mut retained_key_bytes = 0;
        for row in rows {
            super::check_timeout(context.controls)?;
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
                Self::Regression(_) => RegressionAccumulator::retained_heap_bytes(),
                Self::Statistical(state) => state.retained_heap_bytes(),
                _ => 0,
            };
            key_memory.resize(retained_key_bytes, bytes)?;
            retained_key_bytes = bytes;
        }
        accumulator.finish()
    }

    pub(super) fn new(function: &FunctionCall) -> Self {
        if let Some(state) = RegressionAccumulator::new(function) {
            return Self::Regression(state);
        }
        if let Some(state) = StatisticalAccumulator::new(function) {
            return Self::Statistical(state);
        }
        if let Some(state) = BooleanState::new(&function.name) {
            return Self::Boolean(state);
        }
        if let Some(state) = BitwiseState::new(function) {
            return Self::Bitwise(state);
        }
        if is_string_aggregate(function) {
            return Self::StringAgg { value: None };
        }
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

    pub(super) fn update(
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
        let Some(value) = state_input::selected_input(function, row, &value_context)? else {
            return Ok(RetainedChange::default());
        };
        let second_argument = if matches!(self, Self::StringAgg { .. } | Self::Regression(_)) {
            Some(state_input::evaluate_argument(
                function,
                1,
                row,
                &value_context,
            )?)
        } else {
            None
        };
        let key = if matches!(self, Self::MinMax { .. }) {
            function
                .args
                .first()
                .map(|expr| crate::executor::array_order::key(row, expr, &value, user_functions))
                .filter(|key| matches!(key, SemanticValue::Array(_)))
        } else {
            None
        };
        self.update_values(function, value, key, second_argument)
    }

    pub(super) fn update_values(
        &mut self,
        function: &FunctionCall,
        value: Value,
        new_key: Option<SemanticValue>,
        second_argument: Option<Value>,
    ) -> Result<RetainedChange, QueryError> {
        if let Some(change) = super::interval_aggregate::update(self, &value) {
            return change;
        }
        match self {
            Self::Interval(_) => {}
            Self::Statistical(state) => {
                let before = state.retained_heap_bytes();
                state.update(&function.name, &value)?;
                return Ok(RetainedChange {
                    before,
                    after: state.retained_heap_bytes(),
                });
            }
            Self::Regression(state) => {
                let second = second_argument.as_ref().ok_or_else(|| {
                    QueryError::General(format!("{} requires two FLOAT8 inputs", function.name))
                })?;
                state.update(&function.name, &value, second)?;
            }
            Self::Boolean(state) => match value {
                Value::Bool(value) => state.update(value),
                Value::Null => {}
                value => return Err(non_numeric_input(function, &value)),
            },
            Self::Bitwise(state) => state.update(function, value)?,
            Self::Count { count } => {
                if !matches!(value, Value::Null) {
                    *count = count.checked_add(1).ok_or_else(integer_overflow)?;
                }
            }
            Self::StringAgg { value: output } => {
                return update_string_aggregate(output, function, value, second_argument);
            }
            Self::Sum { sum, seen } => update_sum(sum, seen, function, value)?,
            Self::Avg { sum, count } => update_average(sum, count, function, value)?,
            Self::MinMax { selected, key, max } => {
                if matches!(value, Value::Null) {
                    return Ok(RetainedChange::default());
                }
                let replace = selected.as_ref().is_none_or(|current| {
                    let ordering = key.as_ref().zip(new_key.as_ref()).map_or_else(
                        || compare_values(&value, current),
                        |(current, new)| new.cmp(current),
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
                        &value,
                        key.as_ref(),
                        new_key.as_ref(),
                    );
                    *selected = Some(value);
                    *key = new_key;
                    return Ok(change);
                }
            }
        }
        Ok(RetainedChange::default())
    }

    fn merge(&mut self, other: &Self) -> Result<RetainedChange, QueryError> {
        match (self, other) {
            (Self::Boolean(state), Self::Boolean(later)) => state.merge(later)?,
            (Self::Bitwise(state), Self::Bitwise(later)) => state.merge(later)?,
            (Self::Interval(state), Self::Interval(later)) => state.merge(later)?,
            (Self::Regression(state), Self::Regression(later)) => state.merge(later)?,
            (Self::Statistical(state), Self::Statistical(later)) => {
                let before = state.retained_heap_bytes();
                state.merge(later)?;
                return Ok(RetainedChange {
                    before,
                    after: state.retained_heap_bytes(),
                });
            }
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
                sum.merge(other_sum)?;
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
            Self::Boolean(state) => state.finish(),
            Self::Bitwise(state) => state.finish(),
            Self::Count { count } => Value::Int64(count),
            Self::StringAgg { value } => value.map_or(Value::Null, Value::String),
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
                    sum.finish_mean(count)?
                }
            }
            Self::MinMax { selected, .. } => selected.unwrap_or(Value::Null),
            Self::Interval(state) => state.finish()?,
            Self::Regression(state) => state.finish(),
            Self::Statistical(state) => state.finish()?,
        })
    }

    /// True when this accumulator folded a float, so partitions of it cannot
    /// be merged without re-associating the row-order sum.
    pub(super) fn requires_row_order(&self) -> bool {
        match self {
            Self::Sum { sum, .. } => sum.requires_row_order(),
            Self::Avg { sum, .. } => sum.requires_row_order(),
            Self::Regression(_) => RegressionAccumulator::requires_row_order(),
            Self::Statistical(state) => state.requires_row_order(),
            Self::StringAgg { .. } => true,
            Self::Boolean(_)
            | Self::Bitwise(_)
            | Self::Count { .. }
            | Self::MinMax { .. }
            | Self::Interval(_) => false,
        }
    }

    pub(super) fn retained_heap_bytes(&self) -> Result<usize, QueryError> {
        Ok(match self {
            Self::MinMax {
                selected: Some(value),
                key,
                ..
            } => crate::executor::retained_memory::add(
                crate::executor::retained_memory::value_clone_bytes(value)?,
                key.as_ref().map_or(0, SemanticValue::estimated_bytes),
            )?,
            Self::StringAgg { value: Some(value) } => value.len(),
            Self::Regression(_) => RegressionAccumulator::retained_heap_bytes(),
            Self::Statistical(state) => state.retained_heap_bytes(),
            _ => 0,
        })
    }
}

fn update_sum(
    sum: &mut NumericSum,
    seen: &mut bool,
    function: &FunctionCall,
    value: Value,
) -> Result<(), QueryError> {
    match value {
        Value::Int64(value) => {
            sum.add_int(value);
            *seen = true;
        }
        Value::Typed(TypedValue::Numeric(value)) => {
            sum.add_numeric(&value)?;
            *seen = true;
        }
        Value::Typed(TypedValue::Real(value)) => {
            sum.add_real(value);
            *seen = true;
        }
        Value::Float64(value) => {
            sum.add_float(value);
            *seen = true;
        }
        Value::Null => {}
        value => return Err(non_numeric_input(function, &value)),
    }
    Ok(())
}

fn update_average(
    sum: &mut AvgSum,
    count: &mut usize,
    function: &FunctionCall,
    value: Value,
) -> Result<(), QueryError> {
    match value {
        Value::Int64(value) => {
            sum.add_int(value);
            *count += 1;
        }
        Value::Typed(TypedValue::Numeric(value)) => {
            sum.add_numeric(&value)?;
            *count += 1;
        }
        Value::Typed(TypedValue::Real(value)) => {
            sum.add_float(f64::from(value));
            *count += 1;
        }
        Value::Float64(value) => {
            sum.add_float(value);
            *count += 1;
        }
        Value::Null => {}
        value => return Err(non_numeric_input(function, &value)),
    }
    Ok(())
}

fn is_string_aggregate(function: &FunctionCall) -> bool {
    crate::sql::functions::function(&function.name)
        .is_some_and(|metadata| metadata.id == crate::sql::functions::FunctionId::StringAgg)
}

pub(super) fn invalid_string_aggregate_input(function: &FunctionCall) -> QueryError {
    QueryError::General(format!(
        "function {}(text, text) does not exist",
        function.name.to_ascii_lowercase()
    ))
}

fn string_aggregate_size_error() -> QueryError {
    crate::app::CassieError::ResourceLimit(
        "STRING_AGG result exceeds the query memory limit".into(),
    )
    .into()
}

fn update_string_aggregate(
    output: &mut Option<String>,
    function: &FunctionCall,
    value: Value,
    delimiter: Option<Value>,
) -> Result<RetainedChange, QueryError> {
    if matches!(value, Value::Null) {
        return Ok(RetainedChange::default());
    }
    let Value::String(value) = value else {
        return Err(invalid_string_aggregate_input(function));
    };
    let delimiter = match delimiter {
        Some(Value::String(delimiter)) => delimiter,
        Some(Value::Null) | None => String::new(),
        Some(_) => return Err(invalid_string_aggregate_input(function)),
    };
    let had_value = output.is_some();
    let separator_bytes = if had_value { delimiter.len() } else { 0 };
    let additional_bytes = separator_bytes
        .checked_add(value.len())
        .ok_or_else(string_aggregate_size_error)?;
    let output = output.get_or_insert_with(String::new);
    let before = output.len();
    output
        .try_reserve(additional_bytes)
        .map_err(|_| string_aggregate_size_error())?;
    if had_value {
        output.push_str(&delimiter);
    }
    output.push_str(&value);
    Ok(RetainedChange {
        before,
        after: output.len(),
    })
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
    /// Exact `numeric` inputs (and integers folded into them).
    Exact(Numeric),
    /// `sum(real)` accumulates in single precision, as `float4pl` does.
    Real {
        sum: f32,
        overflow: bool,
    },
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
            Self::Exact(sum) => {
                if let Ok(total) = numeric_math::add(sum, &Numeric::from_i64(value)) {
                    *sum = total;
                }
            }
            Self::Float { .. } | Self::Real { .. } => self.add_float(i64_to_f64(value)),
        }
    }

    pub(super) fn add_numeric(&mut self, value: &Numeric) -> Result<(), QueryError> {
        match self {
            Self::Int(sum) => {
                let total = Numeric::from_i128(sum.total);
                *self = Self::Exact(numeric_math::add(&total, value).map_err(numeric_error)?);
            }
            Self::Exact(sum) => *sum = numeric_math::add(sum, value).map_err(numeric_error)?,
            Self::Float { .. } | Self::Real { .. } => self.add_float(value.to_f64()),
        }
        Ok(())
    }

    pub(super) fn add_real(&mut self, value: f32) {
        match self {
            Self::Int(sum) if sum.total == 0 && sum.prefix_min == 0 && sum.prefix_max == 0 => {
                *self = Self::Real {
                    sum: value,
                    overflow: false,
                };
            }
            Self::Real { sum, overflow } => {
                let total = *sum + value;
                *overflow |= total.is_infinite() && sum.is_finite() && value.is_finite();
                *sum = total;
            }
            _ => self.add_float(f64::from(value)),
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
        match self {
            Self::Int(sum) => {
                *self = Self::Float {
                    sum: i128_to_f64(sum.total),
                    overflow: sum.overflowed().then_some(SumOverflow::Integer),
                };
            }
            Self::Exact(sum) => {
                *self = Self::Float {
                    sum: sum.to_f64(),
                    overflow: None,
                };
            }
            Self::Real { sum, overflow } => {
                *self = Self::Float {
                    sum: f64::from(*sum),
                    overflow: overflow.then_some(SumOverflow::Float),
                };
            }
            Self::Float { .. } => {}
        }
    }

    /// True once a float participated: the state is then a row-order `f64`
    /// fold that a partition merge cannot reproduce.
    pub(super) fn requires_row_order(&self) -> bool {
        matches!(self, Self::Float { .. } | Self::Real { .. })
    }

    /// Appends the state of the rows that follow this one. Exact for
    /// integer states; float states must be folded in row order instead.
    pub(super) fn merge(&mut self, later: &Self) -> Result<(), QueryError> {
        match (&mut *self, later) {
            (Self::Int(sum), Self::Int(later)) => sum.append(later),
            (Self::Exact(_), Self::Int(later)) => {
                self.add_numeric(&Numeric::from_i128(later.total))?;
            }
            (Self::Int(_) | Self::Exact(_), Self::Exact(later)) => self.add_numeric(later)?,
            (Self::Real { .. } | Self::Int(_), Self::Real { sum, .. }) => self.add_real(*sum),
            (_, Self::Int(later)) => self.add_float(i128_to_f64(later.total)),
            (_, Self::Exact(later)) => self.add_float(later.to_f64()),
            (_, Self::Real { sum, .. }) => self.add_float(f64::from(*sum)),
            (_, Self::Float { sum, .. }) => self.add_float(*sum),
        }
        Ok(())
    }

    pub(super) fn finish_value(self) -> Result<Value, QueryError> {
        match self {
            // `sum(int8)` is `numeric` in PostgreSQL, so an exact total
            // beyond the `i64` range is still a result.
            Self::Int(sum) => Ok(i64::try_from(sum.total).map_or_else(
                |_| Value::Typed(TypedValue::Numeric(Numeric::from_i128(sum.total))),
                Value::Int64,
            )),
            Self::Exact(sum) => Ok(Value::Typed(TypedValue::Numeric(sum))),
            Self::Real { overflow: true, .. } => Err(QueryError::Cassie(
                crate::app::CassieError::TypeInput(crate::types::extended::TypeInputError {
                    sqlstate: "22003",
                    message: "value out of range: overflow".to_string(),
                }),
            )),
            Self::Real { sum, .. } => Ok(Value::Typed(TypedValue::Real(sum))),
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
    /// The exact total of `numeric` inputs; integer inputs stay in `exact`.
    numeric: Option<Numeric>,
    exact: IntegerSum,
    saw_float: bool,
    overflowed: bool,
}

impl AvgSum {
    /// Integers up to 2^53 are exact in `f64`; bounding every prefix by
    /// 2^52 also bounds every input by 2^53, so each addition is exact and
    /// any grouping of the fold yields the same value.
    const EXACT_PREFIX: i128 = 1 << 52;

    pub(super) fn add_int(&mut self, value: i64) {
        self.overflowed |= numeric::add_f64_overflowed(&mut self.sum, i64_to_f64(value));
        self.exact.add(value);
    }

    pub(super) fn add_float(&mut self, value: f64) {
        self.overflowed |= numeric::add_f64_overflowed(&mut self.sum, value);
        self.saw_float = true;
    }

    pub(super) fn add_numeric(&mut self, value: &Numeric) -> Result<(), QueryError> {
        self.overflowed |= numeric::add_f64_overflowed(&mut self.sum, value.to_f64());
        let total = self.numeric.take().unwrap_or_else(Numeric::zero);
        self.numeric = Some(numeric_math::add(&total, value).map_err(numeric_error)?);
        Ok(())
    }

    fn merge(&mut self, later: &Self) -> Result<(), QueryError> {
        self.overflowed |=
            numeric::add_f64_overflowed(&mut self.sum, later.sum) || later.overflowed;
        self.exact.append(&later.exact);
        self.saw_float |= later.saw_float;
        if let Some(later) = &later.numeric {
            let total = self.numeric.take().unwrap_or_else(Numeric::zero);
            self.numeric = Some(numeric_math::add(&total, later).map_err(numeric_error)?);
        }
        Ok(())
    }

    /// `avg` of exact inputs is `numeric`; of floating inputs `double precision`.
    pub(super) fn finish_mean(&self, count: usize) -> Result<Value, QueryError> {
        if !self.saw_float {
            let integers = Numeric::from_i128(self.exact.total);
            let total = match &self.numeric {
                Some(numeric) => numeric_math::add(numeric, &integers).map_err(numeric_error)?,
                None => integers,
            };
            let count = Numeric::from_i128(i128::try_from(count).unwrap_or(i128::MAX));
            return numeric_math::divide(&total, &count)
                .map(|mean| Value::Typed(TypedValue::Numeric(mean)))
                .map_err(numeric_error);
        }
        if self.overflowed {
            return Err(SumOverflow::Float.error());
        }
        Ok(Value::Float64(self.sum / usize_to_f64(count)))
    }

    /// True when the fold is not exact, so merged partition sums could
    /// differ from the row-order fold.
    fn requires_row_order(&self) -> bool {
        self.saw_float
            || self.exact.prefix_min < -Self::EXACT_PREFIX
            || self.exact.prefix_max > Self::EXACT_PREFIX
    }
}

fn numeric_error(error: crate::types::extended::TypeInputError) -> QueryError {
    QueryError::Cassie(crate::app::CassieError::TypeInput(error))
}

fn non_numeric_input(function: &FunctionCall, value: &Value) -> QueryError {
    QueryError::General(numeric::non_numeric_aggregate_input(&function.name, value))
}

fn integer_overflow() -> QueryError {
    QueryError::General(String::from("aggregate integer overflow"))
}
