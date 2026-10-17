//! Existing signed integer widths and checked PostgreSQL integer input.
use serde::{Deserialize, Serialize};

use super::DataType;

/// The three existing SQL signed-integer domains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum IntegerWidth {
    I16,
    I32,
    I64,
}

/// A resolved builtin integer arithmetic operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntegerOperation {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

impl IntegerOperation {
    pub(crate) const fn symbol(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
            Self::Rem => "%",
        }
    }
}

impl IntegerWidth {
    pub(crate) fn bounds(self) -> (i64, i64) {
        match self {
            Self::I16 => (i64::from(i16::MIN), i64::from(i16::MAX)),
            Self::I32 => (i64::from(i32::MIN), i64::from(i32::MAX)),
            Self::I64 => (i64::MIN, i64::MAX),
        }
    }

    pub(crate) fn contains(self, value: i64) -> bool {
        let (min, max) = self.bounds();
        (min..=max).contains(&value)
    }

    pub(crate) const fn data_type(self) -> DataType {
        match self {
            Self::I16 => DataType::SmallInt,
            Self::I32 => DataType::Int,
            Self::I64 => DataType::BigInt,
        }
    }

    pub(crate) const fn from_data_type(data_type: &DataType) -> Option<Self> {
        match data_type {
            DataType::SmallInt => Some(Self::I16),
            DataType::Int => Some(Self::I32),
            DataType::BigInt => Some(Self::I64),
            _ => None,
        }
    }

    pub(crate) const fn range_message(self) -> &'static str {
        match self {
            Self::I16 => "smallint out of range",
            Self::I32 => "integer out of range",
            Self::I64 => "bigint out of range",
        }
    }

    pub(crate) const fn syntax_message(self) -> &'static str {
        match self {
            Self::I16 => "invalid input syntax for type smallint",
            Self::I32 => "invalid input syntax for type integer",
            Self::I64 => "invalid input syntax for type bigint",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IntegerInputError {
    Syntax,
    Range,
}

pub(crate) fn parse(input: &str, width: IntegerWidth) -> Result<i64, IntegerInputError> {
    let input = input
        .trim_matches(|character: char| character.is_ascii_whitespace() || character == '\u{b}');
    if let Ok(value) = input.parse::<i64>() {
        return width
            .contains(value)
            .then_some(value)
            .ok_or(IntegerInputError::Range);
    }
    let negative = input.starts_with('-');
    let unsigned = input.strip_prefix(['+', '-']).unwrap_or(input);
    let (digits, radix) = [("0x", "0X", 16), ("0o", "0O", 8), ("0b", "0B", 2)]
        .into_iter()
        .find_map(|(lower, upper, radix)| {
            unsigned
                .strip_prefix(lower)
                .or_else(|| unsigned.strip_prefix(upper))
                .map(|digits| (digits, radix))
        })
        .unwrap_or((unsigned, 10));
    let bytes = digits.as_bytes();
    let mut magnitude = 0_u64;
    let mut has_digit = false;
    let threshold = width.bounds().0.unsigned_abs() / radix;
    for (index, byte) in bytes.iter().copied().enumerate() {
        if byte == b'_' {
            if (index == 0 && radix == 10)
                || bytes
                    .get(index + 1)
                    .is_none_or(|byte| digit(*byte, radix).is_none())
            {
                return Err(IntegerInputError::Syntax);
            }
            continue;
        }
        let value = digit(byte, radix).ok_or(IntegerInputError::Syntax)?;
        // Match numutils.c: early accumulator overflow precedes a later bad
        // tail, but the final signed-domain check follows tail validation.
        if magnitude > threshold {
            return Err(IntegerInputError::Range);
        }
        magnitude = magnitude
            .checked_mul(radix)
            .and_then(|value_so_far| value_so_far.checked_add(value))
            .ok_or(IntegerInputError::Range)?;
        has_digit = true;
    }
    if !has_digit {
        return Err(IntegerInputError::Syntax);
    }
    let value = if negative && magnitude == (1_u64 << 63) {
        i64::MIN
    } else {
        let value = i64::try_from(magnitude).map_err(|_| IntegerInputError::Range)?;
        if negative {
            -value
        } else {
            value
        }
    };
    width
        .contains(value)
        .then_some(value)
        .ok_or(IntegerInputError::Range)
}

fn digit(byte: u8, radix: u64) -> Option<u64> {
    let digit = match byte {
        b'0'..=b'9' => u64::from(byte - b'0'),
        b'a'..=b'f' => u64::from(byte - b'a' + 10),
        b'A'..=b'F' => u64::from(byte - b'A' + 10),
        _ => return None,
    };
    (digit < radix).then_some(digit)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IntegerArithmeticError {
    Range,
    DivisionByZero,
}

pub(crate) fn arithmetic(
    width: IntegerWidth,
    operation: IntegerOperation,
    left: i64,
    right: i64,
) -> Result<i64, IntegerArithmeticError> {
    if !width.contains(left) || !width.contains(right) {
        return Err(IntegerArithmeticError::Range);
    }
    let value = match operation {
        IntegerOperation::Add => left.checked_add(right),
        IntegerOperation::Sub => left.checked_sub(right),
        IntegerOperation::Mul => left.checked_mul(right),
        IntegerOperation::Div | IntegerOperation::Rem if right == 0 => {
            return Err(IntegerArithmeticError::DivisionByZero)
        }
        IntegerOperation::Div => left.checked_div(right),
        IntegerOperation::Rem if right == -1 => Some(0),
        IntegerOperation::Rem => left.checked_rem(right),
    }
    .ok_or(IntegerArithmeticError::Range)?;
    width
        .contains(value)
        .then_some(value)
        .ok_or(IntegerArithmeticError::Range)
}

#[cfg(test)]
#[path = "integer_tests.rs"]
mod tests;

pub(crate) fn negate(width: IntegerWidth, value: i64) -> Result<i64, IntegerArithmeticError> {
    if !width.contains(value) {
        return Err(IntegerArithmeticError::Range);
    }
    let value = value.checked_neg().ok_or(IntegerArithmeticError::Range)?;
    width
        .contains(value)
        .then_some(value)
        .ok_or(IntegerArithmeticError::Range)
}
