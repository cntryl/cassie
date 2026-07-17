use std::cmp::Ordering;
use std::hash::{Hash, Hasher};

use crate::types::Value;

#[derive(Debug, Clone)]
pub(crate) struct SemanticNumber(SemanticNumberRepr);

#[derive(Debug, Clone)]
enum SemanticNumberRepr {
    Integer(i64),
    Float(u64),
}

impl SemanticNumber {
    fn from_integer(value: i64) -> Self {
        Self(SemanticNumberRepr::Integer(value))
    }

    fn from_float(value: f64) -> Self {
        if let Some(integer) = exact_float_integer(value) {
            return Self::from_integer(integer);
        }
        let bits = if value.is_nan() {
            f64::NAN.to_bits()
        } else {
            value.to_bits()
        };
        Self(SemanticNumberRepr::Float(bits))
    }

    fn as_float(bits: u64) -> f64 {
        f64::from_bits(bits)
    }
}

impl PartialEq for SemanticNumber {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for SemanticNumber {}

impl PartialOrd for SemanticNumber {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SemanticNumber {
    fn cmp(&self, other: &Self) -> Ordering {
        match (&self.0, &other.0) {
            (SemanticNumberRepr::Integer(left), SemanticNumberRepr::Integer(right)) => {
                left.cmp(right)
            }
            (SemanticNumberRepr::Float(left), SemanticNumberRepr::Float(right)) => {
                Self::as_float(*left).total_cmp(&Self::as_float(*right))
            }
            (SemanticNumberRepr::Integer(left), SemanticNumberRepr::Float(right)) => {
                compare_integer_float(*left, Self::as_float(*right))
            }
            (SemanticNumberRepr::Float(left), SemanticNumberRepr::Integer(right)) => {
                compare_integer_float(*right, Self::as_float(*left)).reverse()
            }
        }
    }
}

impl Hash for SemanticNumber {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match &self.0 {
            SemanticNumberRepr::Integer(value) => {
                0_u8.hash(state);
                value.hash(state);
            }
            SemanticNumberRepr::Float(bits) => {
                1_u8.hash(state);
                bits.hash(state);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum SemanticValue {
    Null,
    Bool(bool),
    Number(SemanticNumber),
    String(String),
    Vector(Vec<u32>),
    Json(String),
}

impl SemanticValue {
    #[must_use]
    pub(crate) fn from_value(value: &Value) -> Self {
        match value {
            Value::Null => Self::Null,
            Value::Bool(value) => Self::Bool(*value),
            Value::Int64(value) => Self::Number(SemanticNumber::from_integer(*value)),
            Value::Float64(value) => Self::Number(SemanticNumber::from_float(*value)),
            Value::String(value) => Self::String(value.clone()),
            Value::Vector(value) => {
                Self::Vector(value.values.iter().map(|part| part.to_bits()).collect())
            }
            Value::Json(value) => {
                Self::Json(serde_json::to_string(value).unwrap_or_else(|_| String::from("null")))
            }
        }
    }

    #[must_use]
    pub(crate) fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    #[must_use]
    pub(crate) fn estimated_bytes(&self) -> usize {
        match self {
            Self::String(value) | Self::Json(value) => value.len(),
            Self::Vector(value) => value.len().saturating_mul(std::mem::size_of::<u32>()),
            Self::Null | Self::Bool(_) | Self::Number(_) => 0,
        }
    }

    fn rank(&self) -> u8 {
        match self {
            Self::Null => 0,
            Self::Bool(_) => 1,
            Self::Number(_) => 2,
            Self::String(_) => 3,
            Self::Vector(_) => 4,
            Self::Json(_) => 5,
        }
    }
}

impl PartialOrd for SemanticValue {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SemanticValue {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Null, Self::Null) => Ordering::Equal,
            (Self::Bool(left), Self::Bool(right)) => left.cmp(right),
            (Self::Number(left), Self::Number(right)) => left.cmp(right),
            (Self::String(left), Self::String(right)) | (Self::Json(left), Self::Json(right)) => {
                left.cmp(right)
            }
            (Self::Vector(left), Self::Vector(right)) => left.cmp(right),
            _ => self.rank().cmp(&other.rank()),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct SemanticKey(Vec<SemanticValue>);

impl SemanticKey {
    #[must_use]
    pub(crate) fn from_values<'a>(values: impl IntoIterator<Item = &'a Value>) -> Self {
        Self(values.into_iter().map(SemanticValue::from_value).collect())
    }

    #[must_use]
    pub(crate) fn single(value: &Value) -> Self {
        Self(vec![SemanticValue::from_value(value)])
    }

    #[must_use]
    pub(crate) fn estimated_bytes(&self) -> usize {
        self.0
            .iter()
            .map(SemanticValue::estimated_bytes)
            .sum::<usize>()
            .saturating_add(
                self.0
                    .len()
                    .saturating_mul(std::mem::size_of::<SemanticValue>()),
            )
    }
}

#[must_use]
pub(crate) fn compare_values(left: &Value, right: &Value) -> Ordering {
    SemanticValue::from_value(left).cmp(&SemanticValue::from_value(right))
}

#[must_use]
pub(crate) fn compare_numeric_values(left: &Value, right: &Value) -> Option<Ordering> {
    let left = match left {
        Value::Int64(value) => SemanticNumber::from_integer(*value),
        Value::Float64(value) => SemanticNumber::from_float(*value),
        _ => return None,
    };
    let right = match right {
        Value::Int64(value) => SemanticNumber::from_integer(*value),
        Value::Float64(value) => SemanticNumber::from_float(*value),
        _ => return None,
    };
    Some(left.cmp(&right))
}

fn exact_float_integer(value: f64) -> Option<i64> {
    if !value.is_finite() || value.fract() != 0.0 {
        return None;
    }
    let limit = 2.0_f64.powi(63);
    if value < -limit || value >= limit {
        return None;
    }
    format!("{value:.0}").parse::<i64>().ok()
}

fn compare_integer_float(integer: i64, float: f64) -> Ordering {
    if float.is_nan() {
        return if float.is_sign_negative() {
            Ordering::Greater
        } else {
            Ordering::Less
        };
    }
    if float == f64::INFINITY {
        return Ordering::Less;
    }
    if float == f64::NEG_INFINITY {
        return Ordering::Greater;
    }

    let limit = 2.0_f64.powi(63);
    if float >= limit {
        return Ordering::Less;
    }
    if float < -limit {
        return Ordering::Greater;
    }

    let truncated = format!("{:.0}", float.trunc())
        .parse::<i64>()
        .expect("bounded integral float should fit in i64");
    match integer.cmp(&truncated) {
        Ordering::Equal if float.fract().is_sign_positive() && float.fract() != 0.0 => {
            Ordering::Less
        }
        Ordering::Equal if float.fract().is_sign_negative() && float.fract() != 0.0 => {
            Ordering::Greater
        }
        ordering => ordering,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn should_compare_integer_float_boundaries_without_rounding() {
        // Arrange
        let exact = Value::Int64(9_007_199_254_740_993);
        let rounded = Value::Float64(9_007_199_254_740_992.0);

        // Act
        let ordering = compare_values(&exact, &rounded);

        // Assert
        assert_eq!(ordering, Ordering::Greater);
        assert_ne!(SemanticKey::single(&exact), SemanticKey::single(&rounded));
    }

    #[test]
    fn should_normalize_cross_type_integral_numbers_and_zero() {
        // Arrange
        let integer = SemanticKey::single(&Value::Int64(1));
        let float = SemanticKey::single(&Value::Float64(1.0));
        let positive_zero = SemanticKey::single(&Value::Float64(0.0));
        let negative_zero = SemanticKey::single(&Value::Float64(-0.0));
        let mut numeric_keys = HashSet::new();

        // Act
        let integral_equal = integer == float;
        let zeros_equal = positive_zero == negative_zero;
        let inserted_integer = numeric_keys.insert(integer);
        let inserted_equal_float = numeric_keys.insert(float);

        // Assert
        assert!(integral_equal);
        assert!(zeros_equal);
        assert!(inserted_integer);
        assert!(!inserted_equal_float);
        assert_eq!(numeric_keys.len(), 1);
    }
}
