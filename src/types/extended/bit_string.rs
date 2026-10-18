//! `bit(n)` and `bit varying(n)`: input, output, typmod coercion, ordering
//! and binary framing, following PostgreSQL's `varbit.c`.
use serde::{Deserialize, Serialize};

use super::TypeInputError;

/// A bit string; bits are stored left-aligned with zero padding.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BitString {
    len: usize,
    bytes: Vec<u8>,
}

impl BitString {
    #[must_use]
    pub fn from_bits(bits: impl IntoIterator<Item = bool>) -> Self {
        let mut value = Self {
            len: 0,
            bytes: Vec::new(),
        };
        for bit in bits {
            value.push(bit);
        }
        value
    }

    fn push(&mut self, bit: bool) {
        if self.len.is_multiple_of(8) {
            self.bytes.push(0);
        }
        if bit {
            let last = self.bytes.len() - 1;
            self.bytes[last] |= 0x80 >> (self.len % 8);
        }
        self.len += 1;
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[must_use]
    pub fn bit(&self, index: usize) -> bool {
        self.bytes[index / 8] & (0x80 >> (index % 8)) != 0
    }

    pub fn bits(&self) -> impl Iterator<Item = bool> + '_ {
        (0..self.len).map(|index| self.bit(index))
    }

    /// Heap bytes the value retains.
    #[must_use]
    pub const fn heap_bytes(&self) -> usize {
        self.bytes.capacity()
    }

    /// `bit_cmp` order: the common bytes, then the shorter string first.
    #[must_use]
    pub fn order_key(&self) -> (Vec<u8>, usize) {
        (self.bytes.clone(), self.len)
    }

    /// Pads on the right with zeros, or truncates, to exactly `len` bits.
    #[must_use]
    pub fn resized(&self, len: usize) -> Self {
        Self::from_bits((0..len).map(|index| index < self.len && self.bit(index)))
    }

    /// The bits from 1-based `start` for `count` bits (to the end without
    /// a count), as `substring`.
    ///
    /// # Errors
    /// Returns `22011` for a negative count.
    pub fn substring(&self, start: i64, count: Option<i64>) -> Result<Self, TypeInputError> {
        if count.is_some_and(|count| count < 0) {
            return Err(TypeInputError::new(
                "22011",
                "negative substring length not allowed".to_string(),
            ));
        }
        let len = i64::try_from(self.len).unwrap_or(i64::MAX);
        let end = count.map_or(len + 1, |count| start.saturating_add(count).min(len + 1));
        let first = start.max(1);
        if first > len || end <= first {
            return Ok(Self::from_bits([]));
        }
        let first = usize::try_from(first - 1).unwrap_or_default();
        let end = usize::try_from(end - 1).unwrap_or_default();
        Ok(Self::from_bits((first..end).map(|index| self.bit(index))))
    }

    #[must_use]
    pub fn concat(&self, other: &Self) -> Self {
        Self::from_bits(self.bits().chain(other.bits()))
    }

    /// `&`, `|` and `#` over equal-length strings.
    ///
    /// # Errors
    /// Returns `22026` when the lengths differ.
    pub fn combine(
        &self,
        other: &Self,
        verb: &str,
        op: impl Fn(u8, u8) -> u8,
    ) -> Result<Self, TypeInputError> {
        if self.len != other.len {
            return Err(TypeInputError::new(
                "22026",
                format!("cannot {verb} bit strings of different sizes"),
            ));
        }
        Ok(Self {
            len: self.len,
            bytes: self
                .bytes
                .iter()
                .zip(&other.bytes)
                .map(|(left, right)| op(*left, *right))
                .collect(),
        })
    }

    #[must_use]
    pub fn complement(&self) -> Self {
        Self::from_bits(self.bits().map(|bit| !bit))
    }

    /// `<<` for a positive count, `>>` for a negative one; the length is kept.
    #[must_use]
    pub fn shift_left(&self, count: i64) -> Self {
        let len = i64::try_from(self.len).unwrap_or(i64::MAX);
        Self::from_bits((0..len).map(|index| {
            let source = index.saturating_add(count);
            (0..len).contains(&source) && self.bit(usize::try_from(source).unwrap_or_default())
        }))
    }

    /// `position(substring IN self)`: 1-based, 0 when absent.
    #[must_use]
    pub fn position(&self, needle: &Self) -> i64 {
        if needle.is_empty() {
            return 1;
        }
        if needle.len > self.len {
            return 0;
        }
        (0..=self.len - needle.len)
            .find(|start| (0..needle.len).all(|index| self.bit(start + index) == needle.bit(index)))
            .map_or(0, |start| i64::try_from(start + 1).unwrap_or(0))
    }

    /// `overlay(self PLACING replacement FROM start [FOR count])`.
    ///
    /// # Errors
    /// Returns `22011` for a non-positive start or negative count.
    pub fn overlay(
        &self,
        replacement: &Self,
        start: i64,
        count: Option<i64>,
    ) -> Result<Self, TypeInputError> {
        let count = count.unwrap_or_else(|| i64::try_from(replacement.len).unwrap_or(0));
        if start <= 0 || count < 0 {
            return Err(TypeInputError::new(
                "22011",
                "negative substring length not allowed".to_string(),
            ));
        }
        let head = self.substring(1, Some(start - 1))?;
        let tail = self.substring(start.saturating_add(count), None)?;
        Ok(head.concat(replacement).concat(&tail))
    }

    #[must_use]
    pub fn count_ones(&self) -> i64 {
        self.bytes
            .iter()
            .map(|byte| i64::from(byte.count_ones()))
            .sum()
    }

    fn index(&self, index: i64) -> Result<usize, TypeInputError> {
        usize::try_from(index)
            .ok()
            .filter(|index| *index < self.len)
            .ok_or_else(|| {
                TypeInputError::new(
                    "2202E",
                    format!(
                        "bit index {index} out of valid range (0..{})",
                        i64::try_from(self.len).unwrap_or(0) - 1
                    ),
                )
            })
    }

    /// `get_bit`, counting from the left.
    ///
    /// # Errors
    /// Returns `2202E` outside the string.
    pub fn get_bit(&self, index: i64) -> Result<i64, TypeInputError> {
        self.index(index).map(|index| i64::from(self.bit(index)))
    }

    /// `set_bit`, counting from the left.
    ///
    /// # Errors
    /// Returns `2202E` outside the string and `22023` for a new bit other
    /// than 0 or 1.
    pub fn set_bit(&self, index: i64, bit: i64) -> Result<Self, TypeInputError> {
        let index = self.index(index)?;
        if !matches!(bit, 0 | 1) {
            return Err(TypeInputError::new(
                "22023",
                "new bit must be 0 or 1".to_string(),
            ));
        }
        Ok(Self::from_bits(self.bits().enumerate().map(
            |(position, value)| if position == index { bit == 1 } else { value },
        )))
    }

    /// `bitfromint8`: the rightmost `len` bits of the two's complement
    /// value, sign-extended on the left.
    #[must_use]
    pub fn from_integer(value: i64, len: usize) -> Self {
        Self::from_bits((0..len).map(|index| {
            let shift = len - 1 - index;
            if shift >= 64 {
                value < 0
            } else {
                (value >> shift) & 1 == 1
            }
        }))
    }

    /// `bittoint4` / `bittoint8`: the bits as a `width`-bit integer.
    ///
    /// # Errors
    /// Returns `22003` when the string is wider than the integer.
    pub fn to_integer(&self, width: u32) -> Result<i64, TypeInputError> {
        let type_name = if width > 32 { "bigint" } else { "integer" };
        if self.len > usize::try_from(width).unwrap_or(64) {
            return Err(TypeInputError::new(
                "22003",
                format!("{type_name} out of range"),
            ));
        }
        let unsigned = self
            .bits()
            .fold(0_u64, |value, bit| (value << 1) | u64::from(bit));
        Ok(if width > 32 {
            i64::from_ne_bytes(unsigned.to_ne_bytes())
        } else {
            let low = u32::try_from(unsigned & u64::from(u32::MAX)).unwrap_or_default();
            i64::from(i32::from_ne_bytes(low.to_ne_bytes()))
        })
    }
}

/// `bit_in` / `varbit_in` without a length modifier: binary digits, or
/// hexadecimal after a leading `x`.
///
/// # Errors
/// Returns `22P02` for an invalid digit.
pub fn parse_text(text: &str) -> Result<BitString, TypeInputError> {
    let (hex, digits) = match text.as_bytes().first() {
        Some(b'x' | b'X') => (true, &text[1..]),
        Some(b'b' | b'B') => (false, &text[1..]),
        _ => (false, text),
    };
    let mut bits = Vec::new();
    for character in digits.chars() {
        if hex {
            let value = character.to_digit(16).ok_or_else(|| {
                TypeInputError::new(
                    "22P02",
                    format!("\"{character}\" is not a valid hexadecimal digit"),
                )
            })?;
            bits.extend((0..4).rev().map(|shift| (value >> shift) & 1 == 1));
        } else {
            bits.push(match character {
                '0' => false,
                '1' => true,
                _ => {
                    return Err(TypeInputError::new(
                        "22P02",
                        format!("\"{character}\" is not a valid binary digit"),
                    ))
                }
            });
        }
    }
    Ok(BitString::from_bits(bits))
}

/// Applies an assignment (implicit) length modifier: `bit(n)` requires
/// exactly `n` bits and `bit varying(n)` at most `n`.
///
/// # Errors
/// Returns `22026` for a length mismatch.
pub fn check_length(
    value: BitString,
    length: Option<u32>,
    varying: bool,
) -> Result<BitString, TypeInputError> {
    let Some(length) = length.and_then(|length| usize::try_from(length).ok()) else {
        return Ok(value);
    };
    if varying && value.len() > length {
        return Err(TypeInputError::new(
            "22026",
            format!("bit string too long for type bit varying({length})"),
        ));
    }
    if !varying && value.len() != length {
        return Err(TypeInputError::new(
            "22026",
            format!(
                "bit string length {} does not match type bit({length})",
                value.len()
            ),
        ));
    }
    Ok(value)
}

/// Applies an explicit (`CAST`) length modifier: `bit(n)` pads or
/// truncates on the right and `bit varying(n)` truncates.
#[must_use]
pub fn coerce_length(value: &BitString, length: Option<u32>, varying: bool) -> BitString {
    match length.and_then(|length| usize::try_from(length).ok()) {
        Some(length) if !varying || value.len() > length => value.resized(length),
        _ => value.clone(),
    }
}

/// Resolves `bit[(n)]`, `bit varying[(n)]` and `varbit[(n)]`; `None` when
/// `base` names another type.
#[must_use]
pub fn parse_type_name(base: &str, modifier: &str) -> Option<Result<super::ExtendedType, String>> {
    let words: Vec<&str> = base.split_whitespace().collect();
    let varying = match words.as_slice() {
        ["bit"] => false,
        ["bit", "varying"] | ["varbit"] => true,
        _ => return None,
    };
    let modifier = modifier.trim();
    let length = if modifier.is_empty() {
        (!varying).then_some(1)
    } else {
        let digits = modifier
            .strip_prefix('(')
            .and_then(|rest| rest.strip_suffix(')'))
            .map(str::trim);
        match digits.and_then(|digits| digits.parse::<u32>().ok()) {
            Some(0) => {
                return Some(Err(format!(
                    "length for type {} must be at least 1",
                    if varying { "varbit" } else { "bit" }
                )))
            }
            Some(length) => Some(length),
            None => return Some(Err(format!("invalid bit string type '{base}{modifier}'"))),
        }
    };
    Some(Ok(if varying {
        super::ExtendedType::VarBit { length }
    } else {
        super::ExtendedType::Bit { length }
    }))
}

#[must_use]
pub fn format_text(value: &BitString) -> String {
    value
        .bits()
        .map(|bit| if bit { '1' } else { '0' })
        .collect()
}

/// `bit_send` / `varbit_send`: an `int4` bit length, then the bytes.
#[must_use]
pub fn send_binary(value: &BitString) -> Vec<u8> {
    let mut out = i32::try_from(value.len())
        .unwrap_or(i32::MAX)
        .to_be_bytes()
        .to_vec();
    out.extend_from_slice(value.bytes());
    out
}

/// `bit_recv` / `varbit_recv`; padding bits are cleared.
///
/// # Errors
/// Returns PostgreSQL's receive error for a negative length or a frame
/// whose byte count does not match it.
pub fn receive_binary(bytes: &[u8]) -> Result<BitString, String> {
    let Some((length, payload)) = bytes.split_first_chunk::<4>() else {
        return Err(super::network::INSUFFICIENT_DATA.to_string());
    };
    let length = usize::try_from(i32::from_be_bytes(*length))
        .map_err(|_| "invalid length in external bit string".to_string())?;
    let needed = length.div_ceil(8);
    if payload.len() < needed {
        return Err(super::network::INSUFFICIENT_DATA.to_string());
    }
    if payload.len() > needed {
        return Err("incorrect binary data format".to_string());
    }
    Ok(BitString {
        len: length,
        bytes: payload.to_vec(),
    }
    .resized(length))
}

#[cfg(test)]
mod tests {
    use super::{check_length, coerce_length, format_text, parse_text, BitString};

    fn bits(text: &str) -> BitString {
        parse_text(text).expect("valid bit string")
    }

    #[test]
    fn should_parse_binary_and_hexadecimal_bit_input() {
        // Arrange
        let inputs = ["10101", "x1F", "Xa0", "b011", ""];

        // Act
        let formatted = inputs.map(|input| format_text(&bits(input)));

        // Assert
        assert_eq!(formatted, ["10101", "00011111", "10100000", "011", ""]);
        assert_eq!(
            parse_text("12").map_err(|error| error.sqlstate),
            Err("22P02")
        );
        assert_eq!(
            parse_text("x1G").map_err(|error| error.sqlstate),
            Err("22P02")
        );
    }

    #[test]
    fn should_coerce_and_check_length_modifiers() {
        // Arrange
        let value = bits("1011");

        // Act
        let padded = format_text(&coerce_length(&bits("10"), Some(3), false));
        let truncated = format_text(&coerce_length(&value, Some(3), true));

        // Assert
        assert_eq!((padded.as_str(), truncated.as_str()), ("100", "101"));
        assert!(check_length(bits("10"), Some(3), false).is_err());
        assert!(check_length(value, Some(3), true).is_err());
    }

    #[test]
    fn should_evaluate_bit_string_functions_like_postgresql() {
        // Arrange
        let value = bits("01010101010101010");

        // Act
        let overlay = value.overlay(&bits("11111"), 2, Some(3)).expect("overlay");
        let substring = bits("110010111111")
            .substring(3, Some(2))
            .expect("substring");
        let shifted = bits("10001").shift_left(3);
        let integer = BitString::from_integer(-44, 12);

        // Assert
        assert_eq!(format_text(&overlay), "0111110101010101010");
        assert_eq!(format_text(&substring), "00");
        assert_eq!(format_text(&shifted), "01000");
        assert_eq!(format_text(&integer), "111111010100");
        assert_eq!(bits("000001101011").position(&bits("010")), 8);
        assert_eq!(
            bits("11111111111111111111111111111111").to_integer(32),
            Ok(-1)
        );
    }

    #[test]
    fn should_order_bit_strings_by_bits_then_length() {
        // Arrange
        let mut values = [bits("10"), bits("1"), bits("00101"), bits("101")];

        // Act
        values.sort_by_key(BitString::order_key);

        // Assert
        let ordered: Vec<String> = values.iter().map(format_text).collect();
        assert_eq!(ordered, ["00101", "1", "10", "101"]);
    }
}
