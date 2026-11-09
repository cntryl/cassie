//! Candidate checked decimal rendering for NUMERIC tie values.
use crate::app::CassieError;
use crate::executor::retained_memory::{add, mul};
use crate::executor::QueryError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::extended::numeric::Numeric;
use num_bigint::Sign;

#[cfg(test)]
#[path = "numeric_controlled_text_tests.rs"]
mod tests;

pub(crate) struct NumericText {
    text: String,
    _memory: QueryMemoryReservation,
}
impl NumericText {
    pub(crate) fn text(&self) -> &str {
        &self.text
    }
}

#[derive(Clone, Copy)]
enum NumericPhase {
    Arithmetic,
    Output,
}

#[cfg(test)]
thread_local! {
    static NEXT_ARITHMETIC_CANCEL: std::cell::RefCell<Option<crate::runtime::QueryCancellationHandle>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn cancel_next_arithmetic(cancellation: crate::runtime::QueryCancellationHandle) {
    NEXT_ARITHMETIC_CANCEL.with(|slot| *slot.borrow_mut() = Some(cancellation));
}

pub(crate) fn format(
    value: &Numeric,
    controls: &QueryExecutionControls,
) -> Result<NumericText, QueryError> {
    format_observing(value, controls, |phase| {
        #[cfg(test)]
        if matches!(phase, NumericPhase::Arithmetic) {
            NEXT_ARITHMETIC_CANCEL.with(|slot| {
                if let Some(cancellation) = slot.borrow_mut().take() {
                    cancellation.cancel();
                }
            });
        }
        #[cfg(not(test))]
        let _ = phase;
    })
}

pub(crate) fn format_integer(
    mantissa: &num_bigint::BigInt,
    controls: &QueryExecutionControls,
) -> Result<NumericText, CassieError> {
    format_finite_observing(mantissa, 0, controls, |_| {}).map_err(|error| match error {
        QueryError::Cassie(error) => error,
        QueryError::General(message) => CassieError::Execution(message),
    })
}

fn overflow() -> CassieError {
    CassieError::ResourceLimit("numeric tie formatting capacity overflow".into())
}

fn format_observing(
    value: &Numeric,
    controls: &QueryExecutionControls,
    progress: impl FnMut(NumericPhase),
) -> Result<NumericText, QueryError> {
    controls.check_interruption()?;
    let (mantissa, scale) = match value {
        Numeric::Finite { mantissa, scale } => (mantissa, *scale),
        Numeric::NaN | Numeric::Infinity | Numeric::NegInfinity => {
            let text = match value {
                Numeric::NaN => "NaN",
                Numeric::Infinity => "Infinity",
                Numeric::NegInfinity => "-Infinity",
                Numeric::Finite { .. } => unreachable!(),
            };
            let memory = controls.reserve_query_memory(text.len())?;
            let mut output = String::new();
            output
                .try_reserve_exact(text.len())
                .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
            if output.capacity() > text.len() {
                return Err(overflow().into());
            }
            output.push_str(text);
            return Ok(NumericText {
                text: output,
                _memory: memory,
            });
        }
    };
    format_finite_observing(mantissa, scale, controls, progress)
}

fn format_finite_observing(
    mantissa: &num_bigint::BigInt,
    scale: u32,
    controls: &QueryExecutionControls,
    mut progress: impl FnMut(NumericPhase),
) -> Result<NumericText, QueryError> {
    controls.check_interruption()?;
    let bits = u128::from(mantissa.bits());
    let decimal_bound = bits.checked_mul(30_103).ok_or_else(overflow)? / 100_000 + 1;
    let decimal_bound = usize::try_from(decimal_bound).map_err(|_| overflow())?;
    let limb_capacity = add(decimal_bound, 18)? / 19;
    let limb_memory =
        controls.reserve_query_memory(mul(limb_capacity, std::mem::size_of::<u64>())?)?;
    let mut limbs = Vec::new();
    limbs
        .try_reserve_exact(limb_capacity)
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if limbs.capacity() > limb_capacity {
        return Err(overflow().into());
    }

    const BASE: u128 = 10_000_000_000_000_000_000;
    let mut steps = 0_usize;
    for word in mantissa.iter_u64_digits().rev() {
        controls.check_interruption()?;
        let mut carry = u128::from(word);
        for digit in &mut limbs {
            let number = (u128::from(*digit) << 64) + carry;
            *digit = u64::try_from(number % BASE).map_err(|_| overflow())?;
            carry = number / BASE;
            steps = steps.wrapping_add(1);
            if steps.is_multiple_of(256) {
                progress(NumericPhase::Arithmetic);
                controls.check_interruption()?;
            }
        }
        while carry != 0 {
            if limbs.len() == limb_capacity {
                return Err(overflow().into());
            }
            limbs.push(u64::try_from(carry % BASE).map_err(|_| overflow())?);
            carry /= BASE;
            steps = steps.wrapping_add(1);
            if steps.is_multiple_of(256) {
                progress(NumericPhase::Arithmetic);
                controls.check_interruption()?;
            }
        }
    }
    if limbs.is_empty() {
        limbs.push(0);
    }
    let highest = *limbs.last().expect("a decimal limb exists");
    let highest_digits = if highest == 0 {
        1
    } else {
        usize::try_from(highest.ilog10() + 1).map_err(|_| overflow())?
    };
    let digits = add(mul(limbs.len() - 1, 19)?, highest_digits)?;
    let scale = usize::try_from(scale).map_err(|_| overflow())?;
    let capacity = add(decimal_bound.max(add(scale, 1)?), 2)?;
    let output_memory = controls.reserve_query_memory(capacity)?;
    let mut text = String::new();
    text.try_reserve_exact(capacity)
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if text.capacity() > capacity {
        return Err(overflow().into());
    }
    let mut written = 0_usize;
    let mut emit = |byte: u8| -> Result<(), QueryError> {
        if text.len() == text.capacity() {
            return Err(overflow().into());
        }
        text.push(char::from(byte));
        written = written.wrapping_add(1);
        if written.is_multiple_of(1024) {
            progress(NumericPhase::Output);
            controls.check_interruption()?;
        }
        Ok(())
    };
    if mantissa.sign() == Sign::Minus {
        emit(b'-')?;
    }
    if scale >= digits && scale != 0 {
        emit(b'0')?;
        emit(b'.')?;
        for _ in 0..scale - digits {
            emit(b'0')?;
        }
    }
    let decimal_at = digits.saturating_sub(scale);
    let mut emitted_digits = 0_usize;
    for (index, word) in limbs.iter().rev().enumerate() {
        let mut bytes = [b'0'; 19];
        let mut word = *word;
        for byte in bytes.iter_mut().rev() {
            *byte = b'0' + u8::try_from(word % 10).map_err(|_| overflow())?;
            word /= 10;
        }
        let start = if index == 0 { 19 - highest_digits } else { 0 };
        for byte in &bytes[start..] {
            if scale > 0 && scale < digits && emitted_digits == decimal_at {
                emit(b'.')?;
            }
            emit(*byte)?;
            emitted_digits += 1;
        }
    }
    drop(emit);
    controls.check_interruption()?;
    drop(limbs);
    drop(limb_memory);
    Ok(NumericText {
        text,
        _memory: output_memory,
    })
}
