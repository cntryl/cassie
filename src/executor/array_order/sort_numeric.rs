//! Checked limb normalization for the scoped finite NUMERIC sort key.
use crate::app::CassieError;
use crate::executor::retained_memory::mul;
use crate::executor::semantic::SemanticValue;
use crate::runtime::QueryExecutionControls;
use crate::types::extended::{numeric, ExtendedType};
use crate::types::typed::{TypedKey, TypedValue};
use crate::types::{DataType, Value};

#[cfg(test)]
#[path = "sort_numeric_tests.rs"]
mod tests;

#[derive(Clone, Copy)]
enum NumericProgress {
    DigitCopy,
    Remainder,
    Division,
}

pub(in crate::executor) fn bound(
    value: &Value,
    data_type: Option<&DataType>,
) -> Result<Option<usize>, CassieError> {
    let (
        Value::Typed(TypedValue::Numeric(numeric::Numeric::Finite { mantissa, .. })),
        Some(DataType::Extended(ExtendedType::Numeric(_))),
    ) = (value, data_type)
    else {
        return Ok(None);
    };
    let words = mantissa.iter_u32_digits().len();
    let rounded = words.checked_add(words % 2).ok_or_else(overflow)?;
    Ok(Some(mul(rounded, std::mem::size_of::<u32>())?))
}

pub(super) fn build(
    value: &Value,
    controls: &QueryExecutionControls,
) -> Result<(SemanticValue, usize), CassieError> {
    build_observing(value, controls, |_| {})
}

fn build_observing(
    value: &Value,
    controls: &QueryExecutionControls,
    mut observe: impl FnMut(NumericProgress),
) -> Result<(SemanticValue, usize), CassieError> {
    let Value::Typed(TypedValue::Numeric(numeric::Numeric::Finite { mantissa, scale })) = value
    else {
        return Err(invalid_shape());
    };
    let count = mantissa.iter_u32_digits().len();
    let output_words = count.checked_add(count % 2).ok_or_else(overflow)?;
    let output_bytes = mul(output_words, std::mem::size_of::<u32>())?;
    let scratch = controls.reserve_query_memory(mul(count, std::mem::size_of::<u32>())?)?;
    let mut digits = Vec::new();
    digits
        .try_reserve_exact(count)
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if digits.capacity() > count {
        return Err(CassieError::ResourceLimit(
            "numeric sort workspace exceeded its admission".into(),
        ));
    }
    for digit in mantissa.iter_u32_digits() {
        controls.check_interruption()?;
        digits.push(digit);
        observe(NumericProgress::DigitCopy);
        controls.check_interruption()?;
    }
    let mut exponent = if digits.is_empty() {
        0
    } else {
        -i64::from(*scale)
    };
    while !digits.is_empty() {
        let mut remainder = 0_u64;
        for digit in digits.iter().rev() {
            controls.check_interruption()?;
            remainder = ((remainder << 32) | u64::from(*digit)) % 10;
            observe(NumericProgress::Remainder);
            controls.check_interruption()?;
        }
        if remainder != 0 {
            break;
        }
        let mut carry = 0_u64;
        for digit in digits.iter_mut().rev() {
            controls.check_interruption()?;
            let current = (carry << 32) | u64::from(*digit);
            *digit = u32::try_from(current / 10).expect("base-2^32 quotient fits a digit");
            carry = current % 10;
            observe(NumericProgress::Division);
            controls.check_interruption()?;
        }
        while digits.last() == Some(&0) {
            controls.check_interruption()?;
            digits.pop();
        }
        exponent = exponent.checked_add(1).ok_or_else(overflow)?;
    }
    controls.check_interruption()?;
    if let Some(integer) = integral_i64(mantissa.sign(), &digits, exponent) {
        drop(digits);
        drop(scratch);
        return Ok((SemanticValue::from_value(&Value::Int64(integer)), 0));
    }
    // num-bigint 0.5.1 allocates its private digit buffer and copies this
    // whole slice here; that copy and allocator excess remain unqualified.
    let normalized = num_bigint::BigInt::from_slice(mantissa.sign(), &digits);
    controls.check_interruption()?;
    drop(digits);
    drop(scratch);
    Ok((
        SemanticValue::Typed(TypedKey::Numeric(numeric::NumericKey::Finite {
            mantissa: normalized,
            exponent,
        })),
        output_bytes,
    ))
}

// Match semantic::integral_numeric without allocating a BigInt or power of ten.
fn integral_i64(sign: num_bigint::Sign, digits: &[u32], exponent: i64) -> Option<i64> {
    let exponent = u32::try_from(exponent).ok().filter(|value| *value <= 19)?;
    if digits.len() > 2 {
        return None;
    }
    let mut magnitude = u64::from(digits.first().copied().unwrap_or(0))
        | (u64::from(digits.get(1).copied().unwrap_or(0)) << 32);
    for _ in 0..exponent {
        magnitude = magnitude.checked_mul(10)?;
    }
    match sign {
        num_bigint::Sign::NoSign => Some(0),
        num_bigint::Sign::Plus => i64::try_from(magnitude).ok(),
        num_bigint::Sign::Minus if magnitude == (1_u64 << 63) => Some(i64::MIN),
        num_bigint::Sign::Minus => i64::try_from(magnitude).ok()?.checked_neg(),
    }
}

fn overflow() -> CassieError {
    CassieError::ResourceLimit("numeric sort key size overflow".into())
}

fn invalid_shape() -> CassieError {
    CassieError::Execution("invalid controlled numeric sort key shape".into())
}
