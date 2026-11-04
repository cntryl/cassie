//! Exact and row-order numeric aggregate accumulator transitions.

use super::QueryError;
use crate::types::extended::numeric::{math as numeric_math, Numeric};
use crate::types::numeric::{self, i128_to_f64, i64_to_f64, usize_to_f64};
use crate::types::typed::TypedValue;
use crate::types::Value;

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

    pub(super) fn merge(&mut self, later: &Self) -> Result<(), QueryError> {
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
    pub(super) fn requires_row_order(&self) -> bool {
        self.saw_float
            || self.exact.prefix_min < -Self::EXACT_PREFIX
            || self.exact.prefix_max > Self::EXACT_PREFIX
    }
}

fn numeric_error(error: crate::types::extended::TypeInputError) -> QueryError {
    QueryError::Cassie(crate::app::CassieError::TypeInput(error))
}

pub(super) fn integer_overflow() -> QueryError {
    QueryError::General(String::from("aggregate integer overflow"))
}
