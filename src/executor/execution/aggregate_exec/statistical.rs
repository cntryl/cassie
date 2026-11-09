//! Streaming variance and standard-deviation state for SQL aggregates.
#[path = "statistical/pg_float.rs"]
mod pg_float;

use crate::app::CassieError;
use crate::sql::ast::{Expr, FunctionCall};
use crate::sql::functions::{self, FunctionId};
use crate::types::extended::numeric::{math as numeric_math, Numeric};
use crate::types::typed::TypedValue;
use crate::types::Value;

use super::QueryError;
use pg_float::{FloatCast, PgFloatMoments};

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
    PgFloat {
        cast: FloatCast,
        state: PgFloatMoments,
    },
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
    #[cfg(test)]
    pub(super) fn new(function: &FunctionCall) -> Option<Self> {
        Self::new_with_arguments(function, &function.args)
    }

    pub(super) fn new_with_arguments(function: &FunctionCall, arguments: &[Expr]) -> Option<Self> {
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
            moments: explicit_float_cast(arguments).map_or(Moments::Empty, |cast| {
                Moments::PgFloat {
                    cast,
                    state: PgFloatMoments::default(),
                }
            }),
            result_family: None,
        })
    }

    pub(super) fn update(&mut self, function: &str, value: &Value) -> Result<(), QueryError> {
        if let Moments::PgFloat { cast, state } = &mut self.moments {
            let value = match (*cast, value) {
                (_, Value::Null) => return Ok(()),
                (FloatCast::Float8, Value::Float64(value)) => *value,
                (FloatCast::Real, Value::Typed(TypedValue::Real(value))) => f64::from(*value),
                _ => {
                    return Err(QueryError::General(
                        "explicit float variance received an unexpected value carrier".into(),
                    ))
                }
            };
            state.update(value)?;
            self.result_family = Some(ResultFamily::Float);
            return Ok(());
        }
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
            Moments::PgFloat { .. } => {
                unreachable!("selected float input is handled before exact update")
            }
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
        self.moments = match (left, &other.moments) {
            (
                Moments::PgFloat { cast, mut state },
                Moments::PgFloat {
                    cast: other_cast,
                    state: right,
                },
            ) if cast == *other_cast => {
                state.merge(right)?;
                Moments::PgFloat { cast, state }
            }
            (Moments::PgFloat { .. }, _) | (_, Moments::PgFloat { .. }) => {
                return Err(QueryError::General(
                    "incompatible statistical partial states".into(),
                ));
            }
            (Moments::Exact(mut left), Moments::Exact(right)) => {
                left.merge(right)?;
                Moments::Exact(left)
            }
            (left, _) => merge_moments(left, other.moments.clone())?,
        };
        Ok(())
    }

    pub(super) fn finish(self) -> Result<Value, QueryError> {
        let value = match self.moments {
            Moments::Empty => return Ok(Value::Null),
            Moments::PgFloat { state, .. } => {
                return Ok(state
                    .result(self.kind.sample, self.kind.standard_deviation)
                    .map_or(Value::Null, Value::Float64))
            }
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
        matches!(self.moments, Moments::Float(_) | Moments::PgFloat { .. })
    }

    /// Conservative transition/finalization workspace, including decimal
    /// alignment, coefficient products, division formatting and sqrt temporaries.
    /// The bound counts coefficient bits as bytes before applying its multiplier.
    pub(super) fn working_bytes(&self, input: Option<&Value>) -> Result<usize, QueryError> {
        use crate::executor::retained_memory::{add, mul};
        let mut units = 128;
        if let Moments::Exact(state) = &self.moments {
            units = add(units, numeric_work_units(&state.sum)?)?;
            units = add(units, numeric_work_units(&state.sum_squares)?)?;
        }
        match input {
            Some(Value::Typed(TypedValue::Numeric(value))) => {
                units = add(units, numeric_work_units(value)?)?;
            }
            Some(Value::Int64(_)) => units = add(units, 64)?,
            Some(_) | None if !matches!(self.moments, Moments::Exact(_)) => return Ok(0),
            _ => {}
        }
        Ok(mul(units, 64)?)
    }

    pub(super) fn retained_heap_bytes(&self) -> usize {
        match &self.moments {
            Moments::Exact(state) => numeric_heap_bytes(&state.sum)
                .saturating_add(numeric_heap_bytes(&state.sum_squares)),
            Moments::Empty | Moments::Float(_) | Moments::PgFloat { .. } => 0,
        }
    }
}

fn explicit_float_cast(arguments: &[Expr]) -> Option<FloatCast> {
    match arguments {
        [Expr::Cast {
            data_type: crate::types::DataType::Float,
            ..
        }] => Some(FloatCast::Float8),
        [Expr::Cast {
            data_type: crate::types::DataType::Extended(crate::types::extended::ExtendedType::Real),
            ..
        }] => Some(FloatCast::Real),
        _ => None,
    }
}

pub(super) fn uses_explicit_float(function: &FunctionCall) -> bool {
    functions::function(&function.name).is_some_and(|metadata| {
        functions::statistical::is_statistical_function(metadata.id)
            && explicit_float_cast(&function.args).is_some()
    })
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
        (Moments::PgFloat { .. }, _) | (_, Moments::PgFloat { .. }) => Err(QueryError::General(
            "incompatible statistical partial states".into(),
        )),
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
        Moments::PgFloat { .. } => unreachable!("selected float states are merged separately"),
    }
}

fn numeric_work_units(value: &Numeric) -> Result<usize, QueryError> {
    let Numeric::Finite { mantissa, scale } = value else {
        return Ok(0);
    };
    let bits = usize::try_from(mantissa.bits()).map_err(|_| {
        CassieError::ResourceLimit("statistical numeric coefficient bound overflow".into())
    })?;
    Ok(crate::executor::retained_memory::add(
        bits,
        usize::try_from(*scale).map_err(|_| {
            CassieError::ResourceLimit("statistical numeric scale bound overflow".into())
        })?,
    )?)
}

fn numeric_heap_bytes(value: &Numeric) -> usize {
    match value {
        Numeric::Finite { mantissa, .. } => {
            usize::try_from(mantissa.bits().div_ceil(8)).unwrap_or(usize::MAX)
        }
        Numeric::NaN | Numeric::Infinity | Numeric::NegInfinity => 0,
    }
}

fn count_overflow() -> QueryError {
    QueryError::General("statistical aggregate count overflow".to_owned())
}

fn numeric_error(error: crate::types::extended::TypeInputError) -> QueryError {
    QueryError::Cassie(CassieError::TypeInput(error))
}

#[cfg(test)]
mod tests {
    use super::{numeric_heap_bytes, FunctionId, Moments, ResultFamily, StatisticalAccumulator};
    use crate::sql::ast::FunctionCall;
    use crate::types::extended::numeric::{math as numeric_math, Numeric};
    use crate::types::Value;

    #[test]
    fn should_count_statistical_numeric_magnitudes_at_byte_boundaries() {
        // Arrange
        let cases = [
            (Numeric::from_i64(0), 0),
            (Numeric::from_i64(1), 1),
            (Numeric::from_i64(-255), 1),
            (Numeric::from_i64(256), 2),
            (Numeric::from_i64(-256), 2),
            (Numeric::NaN, 0),
            (Numeric::Infinity, 0),
            (Numeric::NegInfinity, 0),
        ];

        // Act / Assert
        for (value, expected) in cases {
            let actual = numeric_heap_bytes(&value);
            assert_eq!(actual, expected);
            assert_eq!(
                actual,
                crate::types::typed::TypedValue::Numeric(value).heap_bytes()
            );
        }
    }

    #[test]
    fn should_count_exact_statistical_state_after_update_and_merge() {
        // Arrange
        let mut left = accumulator(FunctionId::VarPop);
        let mut right = accumulator(FunctionId::VarPop);
        left.update("var_pop", &Value::Int64(256))
            .expect("first input");
        right
            .update("var_pop", &Value::Int64(-256))
            .expect("second input");

        // Act
        let before = left.retained_heap_bytes();
        left.merge(&right).expect("merge states");

        // Assert
        assert_eq!(before, 5); // 256 uses two bytes; its square uses three.
        assert_eq!(left.retained_heap_bytes(), 3); // Sum is zero; squares total 131072.
    }

    #[test]
    fn should_preserve_right_exact_statistical_state_after_merge() {
        // Arrange
        let mut left = accumulator(FunctionId::VarPop);
        let mut right = accumulator(FunctionId::VarPop);
        left.update("var_pop", &Value::Int64(1))
            .expect("left input");
        right
            .update("var_pop", &Value::Int64(3))
            .expect("right input");
        right
            .update("var_pop", &Value::Int64(5))
            .expect("right input");
        let right_before = right.clone().finish().expect("right result");

        // Act
        left.merge(&right).expect("exact partial merge");

        // Assert
        assert_eq!(
            right.finish().expect("unchanged right result"),
            right_before
        );
        let expected = numeric_math::divide(&Numeric::from_i128(8), &Numeric::from_i128(3))
            .expect("expected variance");
        assert_eq!(
            left.finish().expect("merged result"),
            Value::Typed(crate::types::typed::TypedValue::Numeric(expected))
        );
    }

    #[test]
    fn should_leave_left_moments_empty_when_exact_merge_count_overflows() {
        // Arrange
        let mut left = accumulator(FunctionId::VarPop);
        let mut right = accumulator(FunctionId::VarPop);
        left.update("var_pop", &Value::Int64(1))
            .expect("left input");
        right
            .update("var_pop", &Value::Int64(2))
            .expect("right input");
        let Moments::Exact(state) = &mut left.moments else {
            panic!("exact moments");
        };
        state.count = usize::MAX;

        // Act
        let result = left.merge(&right);

        // Assert
        assert!(result.is_err());
        assert!(matches!(left.moments, Moments::Empty));
        assert!(left.result_family == Some(ResultFamily::Numeric));
        assert!(matches!(right.moments, Moments::Exact(_)));
    }

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

    fn explicit_float_accumulator(name: &str) -> StatisticalAccumulator {
        use crate::sql::ast::{CastIdentity, Expr};
        StatisticalAccumulator::new(&FunctionCall {
            name: name.to_owned(),
            args: vec![Expr::Cast {
                expr: Box::new(Expr::Column("v".to_owned())),
                data_type: crate::types::DataType::Float,
                source_type: None,
                identity: CastIdentity::Scalar,
            }],
            ..FunctionCall::default()
        })
        .expect("statistical accumulator")
    }

    fn explicit_real_accumulator(name: &str) -> StatisticalAccumulator {
        use crate::sql::ast::{CastIdentity, Expr};
        StatisticalAccumulator::new(&FunctionCall {
            name: name.to_owned(),
            args: vec![Expr::Cast {
                expr: Box::new(Expr::Column("v".to_owned())),
                data_type: crate::types::DataType::Extended(
                    crate::types::extended::ExtendedType::Real,
                ),
                source_type: None,
                identity: CastIdentity::Scalar,
            }],
            ..FunctionCall::default()
        })
        .expect("statistical accumulator")
    }

    #[test]
    fn should_use_postgresql_transition_for_explicit_real_inputs() {
        // Arrange
        let mut state = explicit_real_accumulator("var_pop");
        for value in [1.0_f32, 2.0, 4.0] {
            state
                .update(
                    "var_pop",
                    &Value::Typed(crate::types::typed::TypedValue::Real(value)),
                )
                .expect("REAL input");
        }

        // Act / Assert
        assert_eq!(
            state.finish().expect("REAL variance"),
            Value::Float64(1.555_555_555_555_555_6)
        );
    }

    #[test]
    fn should_use_postgresql_rounding_for_explicit_float_variance() {
        // Arrange
        let mut cast = explicit_float_accumulator("var_pop");
        let mut uncast = accumulator(FunctionId::VarPop);
        for value in [1.0, 2.0, 4.0] {
            cast.update("var_pop", &Value::Float64(value))
                .expect("cast input");
            uncast
                .update("var_pop", &Value::Float64(value))
                .expect("uncast input");
        }

        // Act / Assert
        assert_eq!(
            cast.finish().expect("cast result"),
            Value::Float64(1.555_555_555_555_555_6)
        );
        assert_eq!(
            uncast.finish().expect("legacy uncast result"),
            Value::Float64(1.555_555_555_555_555_4)
        );
    }

    #[test]
    fn should_reject_explicit_float_variance_overflow_without_losing_error_order() {
        // Arrange
        let mut overflow = explicit_float_accumulator("var_pop");
        let mut nan_first = explicit_float_accumulator("var_pop");
        nan_first
            .update("var_pop", &Value::Float64(f64::NAN))
            .expect("NaN");

        // Act
        overflow
            .update("var_pop", &Value::Float64(1e308))
            .expect("first finite input");
        let error = overflow
            .update("var_pop", &Value::Float64(1e308))
            .expect_err("overflow");
        nan_first
            .update("var_pop", &Value::Float64(1e308))
            .expect("first after NaN");
        nan_first
            .update("var_pop", &Value::Float64(1e308))
            .expect("second after NaN");

        // Assert
        assert!(
            matches!(error, super::QueryError::Cassie(crate::app::CassieError::TypeInput(ref error)) if error.sqlstate == "22003" && error.message == "value out of range: overflow")
        );
        assert_eq!(
            overflow
                .finish()
                .expect("failed transition keeps prior state"),
            Value::Float64(0.0)
        );
        assert!(
            matches!(nan_first.finish().expect("NaN result"), Value::Float64(value) if value.is_nan())
        );
    }

    #[test]
    fn should_keep_explicit_float_null_and_singleton_infinity_semantics() {
        // Arrange
        let mut population = explicit_float_accumulator("var_pop");
        let mut sample = explicit_float_accumulator("var_samp");
        for state in [&mut population, &mut sample] {
            state.update("variance", &Value::Null).expect("NULL");
            state
                .update("variance", &Value::Float64(f64::INFINITY))
                .expect("infinity");
        }

        // Act / Assert
        assert!(
            matches!(population.finish().expect("population"), Value::Float64(value) if value.is_nan())
        );
        assert_eq!(sample.finish().expect("sample"), Value::Null);
    }

    #[test]
    fn should_keep_explicit_float_state_order_sensitive_and_reject_wrong_carriers() {
        // Arrange
        let mut state = explicit_float_accumulator("var_pop");
        state
            .update("var_pop", &Value::Float64(1.0))
            .expect("first float");

        // Act
        let invalid = state.update("var_pop", &Value::Int64(2));

        // Assert
        assert!(invalid.is_err());
        assert!(state.requires_row_order());
        assert_eq!(
            state.finish().expect("unchanged state"),
            Value::Float64(0.0)
        );
    }

    #[test]
    fn should_preserve_partial_merge_failure_state_for_explicit_float() {
        // Arrange
        let mut left = explicit_float_accumulator("var_pop");
        let mut right = explicit_float_accumulator("var_pop");
        left.update("var_pop", &Value::Float64(1e308))
            .expect("left");
        right
            .update("var_pop", &Value::Float64(1e308))
            .expect("right");

        // Act
        let error = left.merge(&right).expect_err("finite sum overflow");

        // Assert
        assert!(
            matches!(error, super::QueryError::Cassie(crate::app::CassieError::TypeInput(ref error)) if error.sqlstate == "22003")
        );
        assert_eq!(
            left.finish().expect("failed merge leaves no moments"),
            Value::Null
        );
        assert_eq!(
            right.finish().expect("right stays intact"),
            Value::Float64(0.0)
        );
    }

    #[test]
    fn should_reject_merging_different_explicit_float_carriers() {
        // Arrange
        let mut float = explicit_float_accumulator("var_pop");
        let mut real = explicit_real_accumulator("var_pop");
        float
            .update("var_pop", &Value::Float64(1.0))
            .expect("FLOAT8");
        real.update(
            "var_pop",
            &Value::Typed(crate::types::typed::TypedValue::Real(2.0)),
        )
        .expect("REAL");

        // Act
        let result = float.merge(&real);

        // Assert
        assert!(result.is_err());
        assert_eq!(
            float.finish().expect("failed merge leaves no moments"),
            Value::Null
        );
        assert_eq!(
            real.finish().expect("right remains intact"),
            Value::Float64(0.0)
        );
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
