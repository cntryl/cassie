//! Result types of operators and aggregates over the `numeric` and `real`
//! families, following PostgreSQL's operator and aggregate resolution.
use super::ExtendedType;
use crate::types::DataType;

const NUMERIC: DataType = DataType::Extended(ExtendedType::Numeric(None));
const REAL: DataType = DataType::Extended(ExtendedType::Real);

fn is_numeric(data_type: &DataType) -> bool {
    matches!(data_type, DataType::Extended(ExtendedType::Numeric(_)))
}

fn is_real(data_type: &DataType) -> bool {
    matches!(data_type, DataType::Extended(ExtendedType::Real))
}

fn is_integer(data_type: &DataType) -> bool {
    matches!(
        data_type,
        DataType::SmallInt | DataType::Int | DataType::BigInt
    )
}

/// An operand of unknown type (NULL or an untyped literal) adopts the other side.
fn is_unknown(data_type: &DataType) -> bool {
    matches!(data_type, DataType::Null | DataType::Text)
}

/// `+ - * / % ^` when either operand is `numeric` or `real`; `None` otherwise.
#[must_use]
pub fn arithmetic(left: &DataType, right: &DataType, power_or_modulo: bool) -> Option<DataType> {
    let numeric = is_numeric(left) || is_numeric(right);
    let real = is_real(left) || is_real(right);
    if !numeric && !real {
        return None;
    }
    let exact = |data_type: &DataType| {
        is_numeric(data_type) || is_integer(data_type) || is_unknown(data_type)
    };
    if numeric && exact(left) && exact(right) {
        return Some(NUMERIC);
    }
    let single_precision = |data_type: &DataType| is_real(data_type) || is_unknown(data_type);
    if !power_or_modulo && single_precision(left) && single_precision(right) {
        return Some(REAL);
    }
    Some(DataType::Float)
}

/// `sum(argument)`.
#[must_use]
pub fn sum(argument: &DataType) -> DataType {
    match argument {
        DataType::SmallInt | DataType::Int => DataType::BigInt,
        DataType::BigInt => NUMERIC,
        data_type if is_numeric(data_type) => NUMERIC,
        data_type if is_real(data_type) => REAL,
        _ => DataType::Float,
    }
}

/// `avg(argument)` and the other numeric statistical aggregates.
#[must_use]
pub fn avg(argument: &DataType) -> DataType {
    if is_integer(argument) || is_numeric(argument) {
        NUMERIC
    } else {
        DataType::Float
    }
}

/// `var_pop`, `var_samp`, `variance` and the standard-deviation aggregates.
#[must_use]
pub fn statistical(argument: &DataType) -> DataType {
    if is_integer(argument) || is_numeric(argument) {
        NUMERIC
    } else {
        DataType::Float
    }
}

/// The position of a numeric-category type in PostgreSQL's implicit-cast order.
fn numeric_rank(data_type: &DataType) -> Option<u8> {
    match data_type {
        DataType::SmallInt => Some(0),
        DataType::Int => Some(1),
        DataType::BigInt => Some(2),
        DataType::Extended(ExtendedType::Numeric(_)) => Some(3),
        DataType::Extended(ExtendedType::Real) => Some(4),
        DataType::Float => Some(5),
        _ => None,
    }
}

/// The common type of CASE/COALESCE/UNION branches when one is `numeric`
/// or `real`: the branch type every other branch implicitly casts to.
#[must_use]
pub fn common(left: &DataType, right: &DataType) -> Option<DataType> {
    if !(is_numeric(left) || is_numeric(right) || is_real(left) || is_real(right)) {
        return None;
    }
    let (left_rank, right_rank) = (numeric_rank(left)?, numeric_rank(right)?);
    let winner = if left_rank >= right_rank { left } else { right };
    if is_numeric(winner) {
        // Differing typmods resolve to unconstrained numeric.
        return Some(if left == right { left.clone() } else { NUMERIC });
    }
    Some(winner.clone())
}
