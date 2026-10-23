//! Paired FLOAT8 state for PostgreSQL regression aggregates.
use crate::app::CassieError;
use crate::sql::ast::FunctionCall;
use crate::sql::functions::{self, FunctionId};
use crate::types::extended::numeric::Numeric;
use crate::types::numeric::i64_to_f64;
use crate::types::typed::TypedValue;
use crate::types::{FloatInputError, Value};
use num_traits::Zero;

use super::QueryError;

#[derive(Clone, Copy)]
enum Kind {
    Corr,
    CovarPop,
    CovarSamp,
    Count,
    Slope,
    Intercept,
    R2,
    Sxx,
    Sxy,
    Syy,
    AvgX,
    AvgY,
}

/// Youngs-Cramer state in PostgreSQL's `(Y, X)` argument order.
#[derive(Clone)]
pub(super) struct RegressionAccumulator {
    kind: Kind,
    count: i64,
    sum_x: f64,
    sxx: f64,
    sum_y: f64,
    syy: f64,
    sxy: f64,
}

impl RegressionAccumulator {
    pub(super) fn new(function: &FunctionCall) -> Option<Self> {
        let id = functions::function(&function.name)?.id;
        let kind = match id {
            FunctionId::Corr => Kind::Corr,
            FunctionId::CovarPop => Kind::CovarPop,
            FunctionId::CovarSamp => Kind::CovarSamp,
            FunctionId::RegrCount => Kind::Count,
            FunctionId::RegrSlope => Kind::Slope,
            FunctionId::RegrIntercept => Kind::Intercept,
            FunctionId::RegrR2 => Kind::R2,
            FunctionId::RegrSxx => Kind::Sxx,
            FunctionId::RegrSxy => Kind::Sxy,
            FunctionId::RegrSyy => Kind::Syy,
            FunctionId::RegrAvgx => Kind::AvgX,
            FunctionId::RegrAvgy => Kind::AvgY,
            _ => return None,
        };
        Some(Self {
            kind,
            count: 0,
            sum_x: 0.0,
            sxx: 0.0,
            sum_y: 0.0,
            syy: 0.0,
            sxy: 0.0,
        })
    }

    pub(super) fn update(
        &mut self,
        function: &str,
        y_value: &Value,
        x_value: &Value,
    ) -> Result<(), QueryError> {
        if matches!(y_value, Value::Null) || matches!(x_value, Value::Null) {
            return Ok(());
        }
        let y = float8_input(function, y_value)?;
        let x = float8_input(function, x_value)?;
        let next_count = self.count.checked_add(1).ok_or_else(count_overflow)?;
        let previous_n = i64_to_f64(self.count);
        let n = i64_to_f64(next_count);
        let sum_x = self.sum_x + x;
        let sum_y = self.sum_y + y;
        let mut sxx = self.sxx;
        let mut syy = self.syy;
        let mut sxy = self.sxy;

        if self.count > 0 {
            // Keep the operation order used by PostgreSQL's Youngs-Cramer transition.
            let tmp_x = x * n - sum_x;
            let tmp_y = y * n - sum_y;
            let scale = 1.0 / (n * previous_n);
            sxx += tmp_x * tmp_x * scale;
            syy += tmp_y * tmp_y * scale;
            sxy += tmp_x * tmp_y * scale;

            if finite_inputs_overflowed(
                [self.sum_x, self.sum_y],
                [x, y],
                [sum_x, sxx, sum_y, syy, sxy],
            ) {
                return Err(float8_range_error());
            }
            if sxx.is_infinite() {
                sxx = f64::NAN;
            }
            if syy.is_infinite() {
                syy = f64::NAN;
            }
            if sxy.is_infinite() {
                sxy = f64::NAN;
            }
        } else {
            if !x.is_finite() {
                sxx = f64::NAN;
                sxy = f64::NAN;
            }
            if !y.is_finite() {
                syy = f64::NAN;
                sxy = f64::NAN;
            }
        }

        self.count = next_count;
        self.sum_x = sum_x;
        self.sxx = sxx;
        self.sum_y = sum_y;
        self.syy = syy;
        self.sxy = sxy;
        Ok(())
    }

    pub(super) fn merge(&mut self, other: &Self) -> Result<(), QueryError> {
        if other.count == 0 {
            return Ok(());
        }
        if self.count == 0 {
            *self = other.clone();
            return Ok(());
        }

        let next_count = self
            .count
            .checked_add(other.count)
            .ok_or_else(count_overflow)?;
        let n1 = i64_to_f64(self.count);
        let n2 = i64_to_f64(other.count);
        let n = i64_to_f64(next_count);
        let delta_x = self.sum_x / n1 - other.sum_x / n2;
        let delta_y = self.sum_y / n1 - other.sum_y / n2;
        let sum_x = self.sum_x + other.sum_x;
        let sum_y = self.sum_y + other.sum_y;
        let sxx = self.sxx + other.sxx + n1 * n2 * delta_x * delta_x / n;
        let syy = self.syy + other.syy + n1 * n2 * delta_y * delta_y / n;
        let sxy = self.sxy + other.sxy + n1 * n2 * delta_x * delta_y / n;

        if (sum_x.is_infinite() && !self.sum_x.is_infinite() && !other.sum_x.is_infinite())
            || (sum_y.is_infinite() && !self.sum_y.is_infinite() && !other.sum_y.is_infinite())
            || (sxx.is_infinite() && !self.sxx.is_infinite() && !other.sxx.is_infinite())
            || (syy.is_infinite() && !self.syy.is_infinite() && !other.syy.is_infinite())
            || (sxy.is_infinite() && !self.sxy.is_infinite() && !other.sxy.is_infinite())
        {
            return Err(float8_range_error());
        }

        self.count = next_count;
        self.sum_x = sum_x;
        self.sxx = sxx;
        self.sum_y = sum_y;
        self.syy = syy;
        self.sxy = sxy;
        Ok(())
    }

    pub(super) fn finish(self) -> Value {
        if matches!(self.kind, Kind::Count) {
            return Value::Int64(self.count);
        }
        if self.count == 0 {
            return Value::Null;
        }
        let n = i64_to_f64(self.count);
        let result = match self.kind {
            Kind::Corr if self.sxx == 0.0 || self.syy == 0.0 => None,
            Kind::Corr => Some(self.sxy / (self.sxx * self.syy).sqrt()),
            Kind::CovarPop => Some(self.sxy / n),
            Kind::CovarSamp if self.count == 1 => None,
            Kind::CovarSamp => Some(self.sxy / (n - 1.0)),
            Kind::Count => unreachable!("count returns before float results"),
            Kind::Slope | Kind::Intercept | Kind::R2 if self.sxx == 0.0 => None,
            Kind::Slope => Some(self.sxy / self.sxx),
            Kind::Intercept => Some((self.sum_y - self.sum_x * self.sxy / self.sxx) / n),
            Kind::R2 if self.syy == 0.0 => Some(1.0),
            Kind::R2 => Some((self.sxy * self.sxy) / (self.sxx * self.syy)),
            Kind::Sxx => Some(self.sxx),
            Kind::Sxy => Some(self.sxy),
            Kind::Syy => Some(self.syy),
            Kind::AvgX => Some(self.sum_x / n),
            Kind::AvgY => Some(self.sum_y / n),
        };
        result.map_or(Value::Null, Value::Float64)
    }

    pub(super) const fn retained_heap_bytes() -> usize {
        0
    }

    /// Floating regression states stay serial so their row order is retained.
    pub(super) const fn requires_row_order() -> bool {
        true
    }
}

fn float8_input(function: &str, value: &Value) -> Result<f64, QueryError> {
    match value {
        Value::Int64(value) => Ok(i64_to_f64(*value)),
        Value::Float64(value) => Ok(*value),
        Value::Typed(TypedValue::Real(value)) => Ok(f64::from(*value)),
        Value::Typed(TypedValue::Numeric(value)) => {
            let converted = value.to_f64();
            if matches!(value, Numeric::Finite { mantissa, .. } if !mantissa.is_zero())
                && (converted == 0.0 || !converted.is_finite())
            {
                return Err(float8_range_error());
            }
            Ok(converted)
        }
        _ => Err(QueryError::General(format!(
            "{function} requires FLOAT8 inputs"
        ))),
    }
}

fn finite_inputs_overflowed(previous: [f64; 2], inputs: [f64; 2], next: [f64; 5]) -> bool {
    ((next[0].is_infinite() || next[1].is_infinite())
        && !previous[0].is_infinite()
        && !inputs[0].is_infinite())
        || ((next[2].is_infinite() || next[3].is_infinite())
            && !previous[1].is_infinite()
            && !inputs[1].is_infinite())
        || (next[4].is_infinite()
            && !previous[0].is_infinite()
            && !inputs[0].is_infinite()
            && !previous[1].is_infinite()
            && !inputs[1].is_infinite())
}

fn count_overflow() -> QueryError {
    QueryError::General("regression aggregate count overflow".into())
}

fn float8_range_error() -> QueryError {
    QueryError::Cassie(CassieError::FloatInput(FloatInputError::Range))
}

#[cfg(test)]
mod tests {
    use super::RegressionAccumulator;
    use crate::sql::ast::FunctionCall;
    use crate::types::Value;

    fn accumulator(name: &str) -> RegressionAccumulator {
        RegressionAccumulator::new(&FunctionCall {
            name: name.to_owned(),
            ..FunctionCall::default()
        })
        .expect("regression accumulator")
    }

    fn update_pair(state: &mut RegressionAccumulator, y: f64, x: f64) {
        state
            .update("regression", &Value::Float64(y), &Value::Float64(x))
            .expect("regression pair");
    }

    #[test]
    fn should_merge_finite_regression_partials() {
        // Arrange
        let mut left = accumulator("regr_slope");
        update_pair(&mut left, 2.0, 1.0);
        update_pair(&mut left, 4.0, 2.0);
        let mut right = accumulator("regr_slope");
        update_pair(&mut right, 6.0, 3.0);

        // Act
        left.merge(&right).expect("merge partial regression states");
        let result = left.finish();

        // Assert
        assert_eq!(result, Value::Float64(2.0));
    }

    #[test]
    fn should_reject_finite_overflow_when_merging_regression_sums() {
        // Arrange
        let mut left = accumulator("regr_slope");
        update_pair(&mut left, 0.0, f64::MAX);
        let mut right = accumulator("regr_slope");
        update_pair(&mut right, 0.0, f64::MAX);
        let mut left_y = accumulator("regr_slope");
        update_pair(&mut left_y, f64::MAX, 0.0);
        let mut right_y = accumulator("regr_slope");
        update_pair(&mut right_y, f64::MAX, 0.0);

        // Act
        let x_result = left.merge(&right);
        let y_result = left_y.merge(&right_y);

        // Assert
        assert!(matches!(
            x_result,
            Err(super::QueryError::Cassie(
                crate::app::CassieError::FloatInput(crate::types::FloatInputError::Range)
            ))
        ));
        assert!(matches!(
            y_result,
            Err(super::QueryError::Cassie(
                crate::app::CassieError::FloatInput(crate::types::FloatInputError::Range)
            ))
        ));
    }

    #[test]
    fn should_skip_a_regression_pair_when_either_input_is_null() {
        // Arrange
        let mut state = accumulator("regr_count");

        // Act
        state
            .update("regr_count", &Value::Float64(2.0), &Value::Null)
            .expect("null x input");
        state
            .update("regr_count", &Value::Null, &Value::Float64(1.0))
            .expect("null y input");

        // Assert
        assert_eq!(state.finish(), Value::Int64(0));
    }
}
