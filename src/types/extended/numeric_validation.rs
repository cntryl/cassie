//! Borrowed, bounded validation of the existing SQL NUMERIC input grammar.
use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

use super::numeric::{MAX_DSCALE, MAX_INTEGER_DIGITS};

#[cfg(test)]
#[path = "numeric_validation_tests.rs"]
mod tests;

#[derive(Debug)]
pub(crate) enum ValidationError {
    Syntax,
    Range,
    Interrupted(CassieError),
}

#[derive(Clone, Copy)]
struct Limits {
    max_integer_digits: usize,
    max_dscale: u32,
}

#[derive(Clone, Copy)]
enum ValidationPhase {
    Scan,
    RadixConversion,
}

pub(crate) fn validate(
    input: &str,
    controls: Option<&QueryExecutionControls>,
) -> Result<(), ValidationError> {
    validate_with_limits_observing(
        input,
        Limits {
            max_integer_digits: MAX_INTEGER_DIGITS,
            max_dscale: MAX_DSCALE,
        },
        controls,
        |_| {},
    )
}

fn checkpoint(
    controls: Option<&QueryExecutionControls>,
    progress: &mut impl FnMut(ValidationPhase),
    phase: ValidationPhase,
) -> Result<(), ValidationError> {
    if let Some(controls) = controls {
        controls
            .check_interruption()
            .map_err(ValidationError::Interrupted)?;
    }
    progress(phase);
    if let Some(controls) = controls {
        controls
            .check_interruption()
            .map_err(ValidationError::Interrupted)?;
    }
    Ok(())
}

fn validate_with_limits_observing(
    input: &str,
    limits: Limits,
    controls: Option<&QueryExecutionControls>,
    mut progress: impl FnMut(ValidationPhase),
) -> Result<(), ValidationError> {
    checkpoint(controls, &mut progress, ValidationPhase::Scan)?;
    let bytes = input.as_bytes();
    let (mut start, mut end) = (0, bytes.len());
    while start < end && (bytes[start].is_ascii_whitespace() || bytes[start] == b'\x0b') {
        if start.is_multiple_of(1024) {
            checkpoint(controls, &mut progress, ValidationPhase::Scan)?;
        }
        start += 1;
    }
    while end > start && (bytes[end - 1].is_ascii_whitespace() || bytes[end - 1] == b'\x0b') {
        if (bytes.len() - end).is_multiple_of(1024) {
            checkpoint(controls, &mut progress, ValidationPhase::Scan)?;
        }
        end -= 1;
    }
    let trimmed = &input[start..end];
    if trimmed.eq_ignore_ascii_case("nan") {
        return Ok(());
    }
    let body = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
    if body.eq_ignore_ascii_case("infinity") || body.eq_ignore_ascii_case("inf") {
        return Ok(());
    }
    let bytes = body.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'0' {
        let radix_bits = match bytes[1].to_ascii_lowercase() {
            b'b' => Some(1),
            b'o' => Some(3),
            b'x' => Some(4),
            _ => None,
        };
        if let Some(bits) = radix_bits {
            let digits = body[2..].strip_prefix('_').unwrap_or(&body[2..]);
            return validate_radix(digits, bits, limits, controls, &mut progress);
        }
    }
    validate_decimal(body, limits, controls, &mut progress)
}

fn digit(byte: u8, bits: u32) -> Option<u8> {
    let value = match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        b'A'..=b'F' => byte - b'A' + 10,
        _ => return None,
    };
    (u32::from(value) < (1_u32 << bits)).then_some(value)
}

fn separator_is_valid(bytes: &[u8], index: usize, bits: u32) -> bool {
    index
        .checked_sub(1)
        .and_then(|before| bytes.get(before))
        .and_then(|value| digit(*value, bits))
        .is_some()
        && bytes
            .get(index + 1)
            .and_then(|value| digit(*value, bits))
            .is_some()
}

fn validate_radix(
    digits: &str,
    bits: u32,
    limits: Limits,
    controls: Option<&QueryExecutionControls>,
    progress: &mut impl FnMut(ValidationPhase),
) -> Result<(), ValidationError> {
    let bytes = digits.as_bytes();
    let mut significant_digits = 0_usize;
    let mut first = None;
    for (index, byte) in bytes.iter().copied().enumerate() {
        if index.is_multiple_of(1024) {
            checkpoint(controls, progress, ValidationPhase::Scan)?;
        }
        if byte == b'_' {
            if !separator_is_valid(bytes, index, bits) {
                return Err(ValidationError::Syntax);
            }
            continue;
        }
        let value = digit(byte, bits).ok_or(ValidationError::Syntax)?;
        if first.is_some() {
            significant_digits = significant_digits
                .checked_add(1)
                .ok_or(ValidationError::Range)?;
        } else if value != 0 {
            first = Some(value);
        }
    }
    if bytes.is_empty() {
        return Err(ValidationError::Syntax);
    }
    let Some(first) = first else {
        return Ok(());
    };
    let first_bits = u32::BITS - u32::from(first).leading_zeros();
    let bit_width = usize::try_from(bits).map_err(|_| ValidationError::Range)?;
    let first_width = usize::try_from(first_bits).map_err(|_| ValidationError::Range)?;
    let bit_length = significant_digits
        .checked_mul(bit_width)
        .and_then(|value| value.checked_add(first_width))
        .ok_or(ValidationError::Range)?;
    let lower = limits
        .max_integer_digits
        .checked_mul(3)
        .ok_or(ValidationError::Range)?;
    let upper = limits
        .max_integer_digits
        .checked_mul(10)
        .and_then(|value| value.checked_add(2))
        .map(|value| value / 3)
        .ok_or(ValidationError::Range)?;
    if bit_length <= lower {
        return Ok(());
    }
    if bit_length > upper {
        return Err(ValidationError::Range);
    }
    exact_radix_digits(bytes, bits, limits.max_integer_digits, controls, progress)
}

struct Limbs {
    values: Vec<u64>,
    _memory: Option<QueryMemoryReservation>,
}

impl Limbs {
    fn new(
        decimal_limit: usize,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<Self, ValidationError> {
        let capacity = decimal_limit / 19 + 1;
        let bytes = capacity
            .checked_mul(std::mem::size_of::<u64>())
            .ok_or(ValidationError::Range)?;
        let memory = controls
            .map(|controls| controls.reserve_query_memory(bytes))
            .transpose()
            .map_err(ValidationError::Interrupted)?;
        let mut values = Vec::new();
        values.try_reserve_exact(capacity).map_err(|error| {
            ValidationError::Interrupted(CassieError::ResourceLimit(format!(
                "unable to allocate numeric validation limbs: {error}"
            )))
        })?;
        if values.capacity() > capacity {
            return Err(ValidationError::Interrupted(CassieError::ResourceLimit(
                "numeric validation allocator exceeded admission".into(),
            )));
        }
        Ok(Self {
            values,
            _memory: memory,
        })
    }
}

fn exact_radix_digits(
    bytes: &[u8],
    bits: u32,
    decimal_limit: usize,
    controls: Option<&QueryExecutionControls>,
    progress: &mut impl FnMut(ValidationPhase),
) -> Result<(), ValidationError> {
    let mut limbs = Limbs::new(decimal_limit, controls)?;
    let mut started = false;
    let mut word = 0_u64;
    let mut used = 0_u32;
    let mut operations = 0_usize;
    for (index, byte) in bytes.iter().copied().enumerate() {
        if index.is_multiple_of(1024) {
            checkpoint(controls, progress, ValidationPhase::Scan)?;
        }
        if byte == b'_' {
            continue;
        }
        let value = digit(byte, bits).expect("complete radix spelling validated");
        if !started && value == 0 {
            continue;
        }
        let width = if started {
            bits
        } else {
            started = true;
            u32::BITS - u32::from(value).leading_zeros()
        };
        for bit in (0..width).rev() {
            word = (word << 1) | u64::from((u32::from(value) >> bit) & 1);
            used += 1;
            if used == 64 {
                add_word(
                    &mut limbs.values,
                    word,
                    used,
                    decimal_limit,
                    controls,
                    progress,
                    &mut operations,
                )?;
                word = 0;
                used = 0;
            }
        }
    }
    if used != 0 {
        add_word(
            &mut limbs.values,
            word,
            used,
            decimal_limit,
            controls,
            progress,
            &mut operations,
        )?;
    }
    Ok(())
}

fn add_word(
    limbs: &mut Vec<u64>,
    word: u64,
    bits: u32,
    decimal_limit: usize,
    controls: Option<&QueryExecutionControls>,
    progress: &mut impl FnMut(ValidationPhase),
    operations: &mut usize,
) -> Result<(), ValidationError> {
    const BASE: u128 = 10_000_000_000_000_000_000;
    let mut carry = u128::from(word);
    if let Some(controls) = controls {
        controls
            .check_interruption()
            .map_err(ValidationError::Interrupted)?;
    }
    for value in limbs.iter_mut() {
        let next = (u128::from(*value) << bits) + carry;
        *value = u64::try_from(next % BASE).map_err(|_| ValidationError::Range)?;
        carry = next / BASE;
        *operations = operations.wrapping_add(1);
        if operations.is_multiple_of(256) {
            checkpoint(controls, progress, ValidationPhase::RadixConversion)?;
        }
    }
    while carry != 0 {
        if limbs.len() == limbs.capacity() {
            return Err(ValidationError::Range);
        }
        limbs.push(u64::try_from(carry % BASE).map_err(|_| ValidationError::Range)?);
        carry /= BASE;
        *operations = operations.wrapping_add(1);
        if *operations == 1 || operations.is_multiple_of(256) {
            checkpoint(controls, progress, ValidationPhase::RadixConversion)?;
        }
    }
    let Some(highest) = limbs.last().copied() else {
        return Ok(());
    };
    let highest_digits =
        usize::try_from(highest.ilog10() + 1).map_err(|_| ValidationError::Range)?;
    let digits = (limbs.len() - 1)
        .checked_mul(19)
        .and_then(|value| value.checked_add(highest_digits))
        .ok_or(ValidationError::Range)?;
    if digits > decimal_limit {
        return Err(ValidationError::Range);
    }
    Ok(())
}

fn validate_decimal(
    body: &str,
    limits: Limits,
    controls: Option<&QueryExecutionControls>,
    progress: &mut impl FnMut(ValidationPhase),
) -> Result<(), ValidationError> {
    let bytes = body.as_bytes();
    let mut exponent_at = None;
    for (index, byte) in bytes.iter().copied().enumerate() {
        if index.is_multiple_of(1024) {
            checkpoint(controls, progress, ValidationPhase::Scan)?;
        }
        if matches!(byte, b'e' | b'E') {
            exponent_at = Some(index);
            break;
        }
    }
    let (number, exponent) = exponent_at.map_or((bytes, None), |index| {
        (&bytes[..index], Some(&bytes[index + 1..]))
    });
    let mut point = false;
    let mut digits = 0_usize;
    let mut fraction = 0_usize;
    let mut significant = 0_usize;
    let mut nonzero = false;
    for (index, byte) in number.iter().copied().enumerate() {
        if index.is_multiple_of(1024) {
            checkpoint(controls, progress, ValidationPhase::Scan)?;
        }
        if byte == b'_' {
            if !separator_is_valid(number, index, 4)
                || !number[index - 1].is_ascii_digit()
                || !number[index + 1].is_ascii_digit()
            {
                return Err(ValidationError::Syntax);
            }
        } else if byte == b'.' && !point {
            point = true;
        } else if byte.is_ascii_digit() {
            digits = digits.checked_add(1).ok_or(ValidationError::Range)?;
            if point {
                fraction = fraction.checked_add(1).ok_or(ValidationError::Range)?;
            }
            nonzero |= byte != b'0';
            if nonzero {
                significant = significant.checked_add(1).ok_or(ValidationError::Range)?;
            }
        } else {
            return Err(ValidationError::Syntax);
        }
    }
    if digits == 0 {
        return Err(ValidationError::Syntax);
    }
    let mut exponent_value = 0_i64;
    if let Some(exponent) = exponent {
        let (negative, unsigned) = match exponent.first() {
            Some(b'-') => (true, &exponent[1..]),
            Some(b'+') => (false, &exponent[1..]),
            _ => (false, exponent),
        };
        if unsigned.is_empty() {
            return Err(ValidationError::Syntax);
        }
        let mut magnitude = 0_i64;
        for (index, byte) in unsigned.iter().copied().enumerate() {
            if index.is_multiple_of(1024) {
                checkpoint(controls, progress, ValidationPhase::Scan)?;
            }
            if byte == b'_' {
                if !separator_is_valid(unsigned, index, 4)
                    || !unsigned[index - 1].is_ascii_digit()
                    || !unsigned[index + 1].is_ascii_digit()
                {
                    return Err(ValidationError::Syntax);
                }
            } else if byte.is_ascii_digit() {
                magnitude = magnitude
                    .checked_mul(10)
                    .and_then(|value| value.checked_add(i64::from(byte - b'0')))
                    .unwrap_or(i64::MAX);
            } else {
                return Err(ValidationError::Syntax);
            }
        }
        if magnitude > i64::from(i32::MAX / 2) {
            return Err(ValidationError::Range);
        }
        exponent_value = if negative { -magnitude } else { magnitude };
    }
    let scale = i64::try_from(fraction)
        .map_err(|_| ValidationError::Range)?
        .checked_sub(exponent_value)
        .ok_or(ValidationError::Range)?;
    if scale > i64::from(limits.max_dscale) {
        return Err(ValidationError::Range);
    }
    if significant != 0 {
        let integer_digits = i64::try_from(significant)
            .map_err(|_| ValidationError::Range)?
            .checked_sub(scale)
            .ok_or(ValidationError::Range)?
            .max(0);
        if usize::try_from(integer_digits).map_err(|_| ValidationError::Range)?
            > limits.max_integer_digits
        {
            return Err(ValidationError::Range);
        }
    }
    Ok(())
}
