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
    Array(Vec<Self>),
    SqlArray {
        dimensions: Vec<(i32, i32)>,
        elements: Vec<Self>,
    },
    Typed(crate::types::typed::TypedKey),
}

impl SemanticValue {
    #[must_use]
    pub(crate) fn from_value(value: &Value) -> Self {
        match value {
            Value::Null => Self::Null,
            Value::Bool(value) => Self::Bool(*value),
            Value::Int64(value) => Self::Number(SemanticNumber::from_integer(*value)),
            Value::Float64(value) => Self::Number(SemanticNumber::from_float(*value)),
            // Canonical timestamps written before the fixed-width form
            // (`...SSZ`) are widened so they order, group, and compare equal
            // by instant alongside fixed-width values, the same way numeric
            // representations are normalized above.
            Value::String(value) => {
                Self::String(crate::types::temporal::timestamp_order_text(value).into_owned())
            }
            Value::Vector(value) => {
                Self::Vector(value.values.iter().map(|part| part.to_bits()).collect())
            }
            Value::Json(value) => {
                Self::Json(serde_json::to_string(value).unwrap_or_else(|_| String::from("null")))
            }
            Value::Array(value) => Self::SqlArray {
                dimensions: value
                    .dimensions()
                    .iter()
                    .map(|dimension| (dimension.length, dimension.lower_bound))
                    .collect(),
                elements: value.elements().iter().map(Self::from_value).collect(),
            },
            // A `real` keys as the equal double, so real/integer/float
            // joins and set operations compare by value.
            Value::Typed(crate::types::typed::TypedValue::Real(value)) => {
                Self::Number(SemanticNumber::from_float(f64::from(*value)))
            }
            Value::Typed(value) => {
                let key = value.key();
                // An integral `numeric` keys like the equal integer, so
                // int/numeric joins, set operations and grouping agree.
                match integral_numeric(&key) {
                    Some(integer) => Self::Number(SemanticNumber::from_integer(integer)),
                    None => Self::Typed(key),
                }
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
            Self::Array(values) => values.iter().fold(
                values.len().saturating_mul(std::mem::size_of::<Self>()),
                |bytes, value| bytes.saturating_add(value.estimated_bytes()),
            ),
            Self::SqlArray {
                dimensions,
                elements,
            } => dimensions
                .len()
                .saturating_mul(std::mem::size_of::<(i32, i32)>())
                .saturating_add(elements.iter().fold(
                    elements.len().saturating_mul(std::mem::size_of::<Self>()),
                    |bytes, value| bytes.saturating_add(value.estimated_bytes()),
                )),
            Self::Null | Self::Bool(_) | Self::Number(_) | Self::Typed(_) => 0,
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
            Self::Array(_) | Self::SqlArray { .. } => 6,
            Self::Typed(_) => 7,
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
            (Self::Typed(left), Self::Typed(right)) => left.cmp(right),
            (Self::Number(number), Self::Typed(crate::types::typed::TypedKey::Numeric(key))) => {
                compare_number_numeric(number, key)
            }
            (Self::Typed(crate::types::typed::TypedKey::Numeric(key)), Self::Number(number)) => {
                compare_number_numeric(number, key).reverse()
            }
            (Self::Array(left), Self::Array(right)) => {
                compare_array_parts(left, right, |left, right| {
                    match (left.is_null(), right.is_null()) {
                        (true, false) => Ordering::Greater,
                        (false, true) => Ordering::Less,
                        _ => left.cmp(right),
                    }
                })
            }
            (
                Self::SqlArray {
                    dimensions: left_dimensions,
                    elements: left_elements,
                },
                Self::SqlArray {
                    dimensions: right_dimensions,
                    elements: right_elements,
                },
            ) => compare_sql_arrays(
                left_dimensions,
                left_elements,
                right_dimensions,
                right_elements,
            ),
            (Self::Array(_), Self::SqlArray { .. }) => Ordering::Less,
            (Self::SqlArray { .. }, Self::Array(_)) => Ordering::Greater,
            _ => self.rank().cmp(&other.rank()),
        }
    }
}

pub(crate) fn compare_array_parts<T>(
    left: &[T],
    right: &[T],
    mut compare: impl FnMut(&T, &T) -> Ordering,
) -> Ordering {
    for (left, right) in left.iter().zip(right) {
        let ordering = compare(left, right);
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    left.len().cmp(&right.len())
}

fn compare_sql_arrays(
    left_dimensions: &[(i32, i32)],
    left_elements: &[SemanticValue],
    right_dimensions: &[(i32, i32)],
    right_elements: &[SemanticValue],
) -> Ordering {
    let elements = compare_array_parts(left_elements, right_elements, |left, right| {
        match (left.is_null(), right.is_null()) {
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            _ => left.cmp(right),
        }
    });
    if elements != Ordering::Equal {
        return elements;
    }
    let ranks = left_dimensions.len().cmp(&right_dimensions.len());
    if ranks != Ordering::Equal {
        return ranks;
    }
    let lengths = compare_array_parts(left_dimensions, right_dimensions, |left, right| {
        left.0.cmp(&right.0)
    });
    if lengths != Ordering::Equal {
        return lengths;
    }
    compare_array_parts(left_dimensions, right_dimensions, |left, right| {
        left.1.cmp(&right.1)
    })
}

#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct SemanticKey(Vec<SemanticValue>);

impl SemanticKey {
    #[cfg(test)]
    pub(crate) fn actual_retained_bytes_for_test(&self) -> usize {
        fn heap(value: &SemanticValue) -> usize {
            match value {
                SemanticValue::String(text) | SemanticValue::Json(text) => text.capacity(),
                SemanticValue::Vector(values) => values.capacity() * std::mem::size_of::<u32>(),
                SemanticValue::Array(values) => {
                    values.capacity() * std::mem::size_of::<SemanticValue>()
                        + values.iter().map(heap).sum::<usize>()
                }
                SemanticValue::SqlArray {
                    dimensions,
                    elements,
                } => {
                    dimensions.capacity() * std::mem::size_of::<(i32, i32)>()
                        + elements.capacity() * std::mem::size_of::<SemanticValue>()
                        + elements.iter().map(heap).sum::<usize>()
                }
                SemanticValue::Null
                | SemanticValue::Bool(_)
                | SemanticValue::Number(_)
                | SemanticValue::Typed(_) => 0,
            }
        }
        self.0.capacity() * std::mem::size_of::<SemanticValue>()
            + self.0.iter().map(heap).sum::<usize>()
    }

    pub(crate) fn from_semantic_values(values: Vec<SemanticValue>) -> Self {
        Self(values)
    }

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

/// The `i64` an integral `numeric` key equals, if any.
fn integral_numeric(key: &crate::types::typed::TypedKey) -> Option<i64> {
    use crate::types::extended::numeric::{pow10, NumericKey};
    let crate::types::typed::TypedKey::Numeric(NumericKey::Finite { mantissa, exponent }) = key
    else {
        return None;
    };
    let exponent = u32::try_from(*exponent)
        .ok()
        .filter(|exponent| *exponent <= 19)?;
    num_traits::ToPrimitive::to_i64(&(mantissa * pow10(exponent)))
}

/// Orders an integer or float against a non-integral or out-of-range
/// `numeric`. Ties between a float and a `numeric` keep the number first so
/// the order stays consistent with equality across the two key families.
fn compare_number_numeric(
    number: &SemanticNumber,
    key: &crate::types::extended::numeric::NumericKey,
) -> Ordering {
    use crate::types::extended::numeric::{Numeric, NumericKey};
    match &number.0 {
        SemanticNumberRepr::Integer(integer) => Numeric::from_i64(*integer).order_key().cmp(key),
        SemanticNumberRepr::Float(bits) => {
            let float = SemanticNumber::as_float(*bits);
            let numeric = match key {
                NumericKey::NegInfinity => f64::NEG_INFINITY,
                NumericKey::Infinity => f64::INFINITY,
                NumericKey::NaN => f64::NAN,
                NumericKey::Finite { mantissa, exponent } => format!("{mantissa}e{exponent}")
                    .parse::<f64>()
                    .unwrap_or(f64::NAN),
            };
            match float.total_cmp(&numeric) {
                Ordering::Equal => Ordering::Less,
                ordering => ordering,
            }
        }
    }
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
    fn should_normalize_equivalent_numeric_representations() {
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

    #[test]
    fn should_order_equivalent_timestamp_formats_by_instant() {
        // Arrange
        let legacy_whole = Value::String("2024-01-01T12:00:00Z".to_string());
        let fixed_whole = Value::String("2024-01-01T12:00:00.000000Z".to_string());
        let fractional = Value::String("2024-01-01T12:00:00.500000Z".to_string());

        // Act
        let whole_before_fraction = compare_values(&legacy_whole, &fractional);
        let fraction_after_whole = compare_values(&fractional, &legacy_whole);
        let same_instant = SemanticKey::single(&legacy_whole) == SemanticKey::single(&fixed_whole);

        // Assert
        assert_eq!(whole_before_fraction, Ordering::Less);
        assert_eq!(fraction_after_whole, Ordering::Greater);
        assert!(same_instant);
    }
}
