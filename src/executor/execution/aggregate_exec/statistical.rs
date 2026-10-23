//! Streaming variance and standard-deviation state for SQL aggregates.
use crate::app::CassieError;
use crate::sql::ast::FunctionCall;
use crate::sql::functions::{self, FunctionId};
use crate::types::extended::numeric::{math as numeric_math, Numeric};
use crate::types::typed::TypedValue;
use crate::types::Value;

use super::QueryError;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ResultFamily {
    Numeric,
    Float,
}

#[derive(Clone, Copy)]
struct Kind {
    sample: bool,
    standard_deviation: bool,
}

#[derive(Clone)]
enum Moments {
    Empty,
    Exact(ExactMoments),
    Float(FloatMoments),
}

#[derive(Clone)]
struct ExactMoments {
    count: usize,
    sum: Numeric,
    sum_squares: Numeric,
}

#[derive(Clone, Copy, Default)]
struct FloatMoments {
    count: usize,
    mean: f64,
    m2: f64,
    nonfinite: bool,
}

#[derive(Clone)]
pub(super) struct StatisticalAccumulator {
    kind: Kind,
    moments: Moments,
    result_family: Option<ResultFamily>,
}

impl StatisticalAccumulator {
    pub(super) fn new(function: &FunctionCall) -> Option<Self> {
        let id = functions::function(&function.name)?.id;
        let kind = match id {
            FunctionId::VarPop => Kind {
                sample: false,
                standard_deviation: false,
            },
            FunctionId::VarSamp | FunctionId::Variance => Kind {
                sample: true,
                standard_deviation: false,
            },
            FunctionId::StddevPop => Kind {
                sample: false,
                standard_deviation: true,
            },
            FunctionId::StddevSamp | FunctionId::Stddev => Kind {
                sample: true,
                standard_deviation: true,
            },
            _ => return None,
        };
        Some(Self {
            kind,
            moments: Moments::Empty,
            result_family: None,
        })
    }

    pub(super) fn update(&mut self, function: &str, value: &Value) -> Result<(), QueryError> {
        match value {
            Value::Null => Ok(()),
            Value::Int64(value) => self.update_exact(&Numeric::from_i64(*value)),
            Value::Typed(TypedValue::Numeric(value)) => self.update_numeric(value),
            Value::Typed(TypedValue::Real(value)) => self.update_float(f64::from(*value)),
            Value::Float64(value) => self.update_float(*value),
            _ => Err(QueryError::General(format!(
                "{function} requires a numeric input"
            ))),
        }
    }

    fn update_numeric(&mut self, value: &Numeric) -> Result<(), QueryError> {
        if matches!(value, Numeric::Finite { .. }) {
            return self.update_exact(value);
        }
        if self.result_family.is_none() {
            self.result_family = Some(ResultFamily::Numeric);
        }
        self.promote_to_float();
        self.update_float_value(value.to_f64())
    }

    fn update_exact(&mut self, value: &Numeric) -> Result<(), QueryError> {
        if self.result_family.is_none() {
            self.result_family = Some(ResultFamily::Numeric);
        }
        match &mut self.moments {
            Moments::Empty => {
                self.moments = Moments::Exact(ExactMoments::default());
                self.update_exact_value(value)
            }
            Moments::Exact(state) => state.update(value),
            Moments::Float(_) => self.update_float_value(value.to_f64()),
        }
    }

    fn update_float(&mut self, value: f64) -> Result<(), QueryError> {
        self.result_family = Some(ResultFamily::Float);
        self.promote_to_float();
        self.update_float_value(value)
    }

    fn update_exact_value(&mut self, value: &Numeric) -> Result<(), QueryError> {
        let Moments::Exact(state) = &mut self.moments else {
            unreachable!("exact statistical state was initialized")
        };
        state.update(value)
    }

    fn update_float_value(&mut self, value: f64) -> Result<(), QueryError> {
        let Moments::Float(state) = &mut self.moments else {
            self.moments = Moments::Float(FloatMoments::default());
            let Moments::Float(state) = &mut self.moments else {
                unreachable!("floating statistical state was initialized")
            };
            return state.update(value);
        };
        state.update(value)
    }

    fn promote_to_float(&mut self) {
        let moments = std::mem::replace(&mut self.moments, Moments::Empty);
        self.moments = match moments {
            Moments::Exact(state) => Moments::Float(FloatMoments::from_exact(&state)),
            other => other,
        };
    }

    pub(super) fn merge(&mut self, other: &Self) -> Result<(), QueryError> {
        if self.result_family == Some(ResultFamily::Float)
            || other.result_family == Some(ResultFamily::Float)
        {
            self.result_family = Some(ResultFamily::Float);
        } else if self.result_family.is_none() {
            self.result_family = other.result_family;
        }
        let left = std::mem::replace(&mut self.moments, Moments::Empty);
        self.moments = merge_moments(left, other.moments.clone())?;
        Ok(())
    }

    pub(super) fn finish(self) -> Result<Value, QueryError> {
        let value = match self.moments {
            Moments::Empty => return Ok(Value::Null),
            Moments::Exact(state) => {
                let Some(result) = state.result(self.kind)? else {
                    return Ok(Value::Null);
                };
                Value::Typed(TypedValue::Numeric(result))
            }
            Moments::Float(state) => {
                let Some(result) = state.result(self.kind) else {
                    return Ok(Value::Null);
                };
                match self.result_family {
                    Some(ResultFamily::Numeric) => Value::Typed(TypedValue::Numeric(
                        Numeric::from_f64(result).map_err(numeric_error)?,
                    )),
                    Some(ResultFamily::Float) | None => Value::Float64(result),
                }
            }
        };
        Ok(value)
    }

    pub(super) fn requires_row_order(&self) -> bool {
        matches!(self.moments, Moments::Float(_))
    }

    pub(super) fn retained_heap_bytes(&self) -> usize {
        match &self.moments {
            Moments::Exact(state) => numeric_heap_bytes(&state.sum)
                .saturating_add(numeric_heap_bytes(&state.sum_squares)),
            Moments::Empty | Moments::Float(_) => 0,
        }
    }
}

impl Default for ExactMoments {
    fn default() -> Self {
        Self {
            count: 0,
            sum: Numeric::zero(),
            sum_squares: Numeric::zero(),
        }
    }
}

impl ExactMoments {
    fn update(&mut self, value: &Numeric) -> Result<(), QueryError> {
        let next_count = self.count.checked_add(1).ok_or_else(count_overflow)?;
        let square = numeric_math::multiply(value, value).map_err(numeric_error)?;
        let sum = numeric_math::add(&self.sum, value).map_err(numeric_error)?;
        let sum_squares = numeric_math::add(&self.sum_squares, &square).map_err(numeric_error)?;
        self.count = next_count;
        self.sum = sum;
        self.sum_squares = sum_squares;
        Ok(())
    }

    fn merge(&mut self, other: &Self) -> Result<(), QueryError> {
        let count = self
            .count
            .checked_add(other.count)
            .ok_or_else(count_overflow)?;
        let sum = numeric_math::add(&self.sum, &other.sum).map_err(numeric_error)?;
        let sum_squares =
            numeric_math::add(&self.sum_squares, &other.sum_squares).map_err(numeric_error)?;
        self.count = count;
        self.sum = sum;
        self.sum_squares = sum_squares;
        Ok(())
    }

    fn result(&self, kind: Kind) -> Result<Option<Numeric>, QueryError> {
        if self.count == 0 || kind.sample && self.count < 2 {
            return Ok(None);
        }
        let n = Numeric::from_i128(i128::try_from(self.count).map_err(|_| count_overflow())?);
        let sum_squared = numeric_math::multiply(&self.sum, &self.sum).map_err(numeric_error)?;
        let scaled_squares =
            numeric_math::multiply(&self.sum_squares, &n).map_err(numeric_error)?;
        let numerator =
            numeric_math::subtract(&scaled_squares, &sum_squared).map_err(numeric_error)?;
        let denominator = if kind.sample {
            let n_minus_one =
                Numeric::from_i128(i128::try_from(self.count - 1).map_err(|_| count_overflow())?);
            numeric_math::multiply(&n, &n_minus_one).map_err(numeric_error)?
        } else {
            numeric_math::multiply(&n, &n).map_err(numeric_error)?
        };
        let variance = numeric_math::divide(&numerator, &denominator).map_err(numeric_error)?;
        if kind.standard_deviation {
            numeric_math::sqrt(&variance)
                .map(Some)
                .map_err(numeric_error)
        } else {
            Ok(Some(variance))
        }
    }
}

impl FloatMoments {
    fn update(&mut self, value: f64) -> Result<(), QueryError> {
        let count = self.count.checked_add(1).ok_or_else(count_overflow)?;
        self.count = count;
        if !value.is_finite() {
            self.nonfinite = true;
            return Ok(());
        }
        if count == 1 {
            self.mean = value;
            return Ok(());
        }
        let delta = value - self.mean;
        self.mean += delta / crate::types::numeric::usize_to_f64(count);
        self.m2 += delta * (value - self.mean);
        Ok(())
    }

    fn merge(&mut self, other: &Self) -> Result<(), QueryError> {
        if other.count == 0 {
            return Ok(());
        }
        if self.count == 0 {
            *self = *other;
            return Ok(());
        }
        let count = self
            .count
            .checked_add(other.count)
            .ok_or_else(count_overflow)?;
        let left_count = crate::types::numeric::usize_to_f64(self.count);
        let right_count = crate::types::numeric::usize_to_f64(other.count);
        let total = crate::types::numeric::usize_to_f64(count);
        let delta = other.mean - self.mean;
        self.mean += delta * right_count / total;
        self.m2 += other.m2 + delta * delta * left_count * right_count / total;
        self.count = count;
        self.nonfinite |= other.nonfinite;
        Ok(())
    }

    fn from_exact(state: &ExactMoments) -> Self {
        if state.count == 0 {
            return Self::default();
        }
        let count = crate::types::numeric::usize_to_f64(state.count);
        let sum = state.sum.to_f64();
        let mean = sum / count;
        let m2 = state.sum_squares.to_f64() - sum * mean;
        Self {
            count: state.count,
            mean,
            m2,
            nonfinite: !mean.is_finite() || !m2.is_finite(),
        }
    }

    fn result(self, kind: Kind) -> Option<f64> {
        if self.count == 0 || kind.sample && self.count < 2 {
            return None;
        }
        let result = if self.nonfinite {
            f64::NAN
        } else {
            let denominator = if kind.sample {
                crate::types::numeric::usize_to_f64(self.count - 1)
            } else {
                crate::types::numeric::usize_to_f64(self.count)
            };
            self.m2 / denominator
        };
        Some(if kind.standard_deviation {
            result.sqrt()
        } else {
            result
        })
    }
}

fn merge_moments(left: Moments, right: Moments) -> Result<Moments, QueryError> {
    match (left, right) {
        (Moments::Empty, right) | (right, Moments::Empty) => Ok(right),
        (Moments::Exact(mut left), Moments::Exact(right)) => {
            left.merge(&right)?;
            Ok(Moments::Exact(left))
        }
        (left, right) => {
            let mut left = as_float(left);
            left.merge(&as_float(right))?;
            Ok(Moments::Float(left))
        }
    }
}

fn as_float(moments: Moments) -> FloatMoments {
    match moments {
        Moments::Empty => FloatMoments::default(),
        Moments::Float(state) => state,
        Moments::Exact(state) => FloatMoments::from_exact(&state),
    }
}

fn numeric_heap_bytes(value: &Numeric) -> usize {
    TypedValue::Numeric(value.clone()).heap_bytes()
}

fn count_overflow() -> QueryError {
    QueryError::General("statistical aggregate count overflow".to_owned())
}

fn numeric_error(error: crate::types::extended::TypeInputError) -> QueryError {
    QueryError::Cassie(CassieError::TypeInput(error))
}

#[cfg(test)]
mod tests {
    use super::{FunctionId, StatisticalAccumulator};
    use crate::sql::ast::FunctionCall;
    use crate::types::extended::numeric::{math as numeric_math, Numeric};
    use crate::types::Value;

    fn accumulator(id: FunctionId) -> StatisticalAccumulator {
        let name = match id {
            FunctionId::VarPop => "var_pop",
            _ => panic!("unsupported test aggregate"),
        };
        StatisticalAccumulator::new(&FunctionCall {
            name: name.to_owned(),
            ..FunctionCall::default()
        })
        .expect("statistical accumulator")
    }

    #[test]
    fn should_merge_exact_population_variance_partials() {
        // Arrange
        let mut left = accumulator(FunctionId::VarPop);
        left.update("var_pop", &Value::Int64(1))
            .expect("first partial input");
        left.update("var_pop", &Value::Int64(3))
            .expect("second partial input");
        let mut right = accumulator(FunctionId::VarPop);
        right
            .update("var_pop", &Value::Int64(5))
            .expect("third partial input");

        // Act
        left.merge(&right).expect("merge partial moments");
        let result = left.finish().expect("population variance result");
        let expected = numeric_math::divide(&Numeric::from_i128(8), &Numeric::from_i128(3))
            .expect("expected population variance");

        // Assert
        assert_eq!(
            result,
            Value::Typed(crate::types::typed::TypedValue::Numeric(expected))
        );
    }
}
