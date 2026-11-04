//! Sparse finite JSON numbers: native numeric identity without decimal expansion.
use std::cmp::Ordering;
use std::hash::{Hash, Hasher};

use num_bigint::BigInt;

use crate::app::CassieError;
use crate::runtime::QueryExecutionControls;
use crate::types::extended::numeric::{decimal, NumericKey};
use crate::types::extended::TypeInputError;

#[derive(Debug, Clone)]
pub(super) struct Number {
    native: NumericKey,
    digits: String,
    sign: i8,
    weight: i64,
}

impl Number {
    // Input is a serde_json Number, so JSON grammar (including finite-only) is validated.
    pub(super) fn from_validated(
        text: &str,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<Self, CassieError> {
        let parts = decimal::json_parts(text)?;
        let negative = parts.negative;
        let scale = parts.shape.scale;
        let mut digits = String::with_capacity(parts.integer.len() + parts.fraction.len());
        for (index, byte) in parts
            .integer
            .bytes()
            .chain(parts.fraction.bytes())
            .enumerate()
        {
            if index % 256 == 0 {
                check(controls)?;
            }
            digits.push(char::from(byte));
        }
        let first = digits.bytes().position(|byte| byte != b'0');
        let Some(first) = first else {
            return Ok(Self {
                native: NumericKey::Finite {
                    mantissa: BigInt::from(0),
                    exponent: 0,
                },
                digits: String::new(),
                sign: 0,
                weight: 0,
            });
        };
        let end = digits.trim_end_matches('0').len();
        let tail = digits.len() - end;
        let canonical_exponent = -scale + i64::try_from(tail).map_err(|_| overflow())?;
        digits.drain(end..);
        digits.drain(..first);
        let weight = i64::try_from(digits.len()).map_err(|_| overflow())? + canonical_exponent;
        let magnitude = coefficient_integer(&digits, controls)?;
        let mantissa = if negative { -magnitude } else { magnitude };
        Ok(Self {
            native: NumericKey::Finite {
                mantissa,
                exponent: canonical_exponent,
            },
            digits,
            sign: if negative { -1 } else { 1 },
            weight,
        })
    }

    pub(super) fn heap_bytes(&self) -> usize {
        // BigInt's base-2^32 backing fits below one byte per decimal digit plus one limb.
        self.digits
            .capacity()
            .saturating_add(self.digits.len())
            .saturating_add(64)
    }
}

impl PartialEq for Number {
    fn eq(&self, other: &Self) -> bool {
        self.native == other.native
    }
}
impl Eq for Number {}
impl Hash for Number {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.native.hash(state);
    }
}
impl PartialOrd for Number {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Number {
    fn cmp(&self, other: &Self) -> Ordering {
        let sign = self.sign.cmp(&other.sign);
        if !sign.is_eq() || self.sign == 0 {
            return sign;
        }
        let magnitude = self.weight.cmp(&other.weight).then_with(|| {
            self.digits
                .bytes()
                .chain(std::iter::repeat(b'0'))
                .zip(other.digits.bytes().chain(std::iter::repeat(b'0')))
                .take(self.digits.len().max(other.digits.len()))
                .find_map(|(left, right)| (left != right).then_some(left.cmp(&right)))
                .unwrap_or(Ordering::Equal)
        });
        if self.sign < 0 {
            magnitude.reverse()
        } else {
            magnitude
        }
    }
}

fn overflow() -> CassieError {
    TypeInputError::new("22003", "value overflows numeric format".into()).into()
}

// Small-radix chunks preserve interruption boundaries during coefficient construction.
// The lease admits digit text and old/new BigInt limb backing before this loop.
fn coefficient_integer(
    digits: &str,
    controls: Option<&QueryExecutionControls>,
) -> Result<BigInt, CassieError> {
    let first = (digits.len() - 1) % 9 + 1;
    let (head, tail) = digits.as_bytes().split_at(first);
    let chunk_value = |chunk: &[u8]| {
        chunk
            .iter()
            .fold(0_u32, |value, byte| value * 10 + u32::from(byte - b'0'))
    };
    let mut value = BigInt::from(chunk_value(head));
    for chunk in tail.chunks_exact(9) {
        check(controls)?;
        value *= 1_000_000_000_u32;
        value += chunk_value(chunk);
    }
    check(controls)?;
    Ok(value)
}
fn check(controls: Option<&QueryExecutionControls>) -> Result<(), CassieError> {
    controls.map_or(Ok(()), QueryExecutionControls::check_interruption)
}
