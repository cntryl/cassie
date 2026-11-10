//! Exact decimal/binary midpoint equality without a big-integer allocation.

pub(super) fn is_midpoint(value: f64, decimal: &str) -> bool {
    let Some(decimal) = decimal_dyadic(decimal) else {
        return false;
    };
    let bits = value.abs().to_bits();
    let own = binary_dyadic(bits);
    [bits - 1, bits + 1].into_iter().any(|neighbor| {
        if neighbor >> 52 == 0x7ff {
            return false;
        }
        let neighbor = binary_dyadic(neighbor);
        let common = own.1.min(neighbor.1);
        let sum = (own.0 << (own.1 - common)) + (neighbor.0 << (neighbor.1 - common));
        normalize(sum, common - 1) == decimal
    })
}

fn decimal_dyadic(decimal: &str) -> Option<(u128, i32)> {
    let (mantissa, exponent) = decimal.split_once('e')?;
    let mantissa = mantissa.strip_prefix('-').unwrap_or(mantissa);
    let fraction = mantissa
        .split_once('.')
        .map_or(0, |(_, fraction)| fraction.len());
    let mut exponent = exponent.parse::<i32>().ok()? - i32::try_from(fraction).ok()?;
    let mut numerator = decimal_digits(mantissa)?;
    while numerator % 10 == 0 {
        numerator /= 10;
        exponent += 1;
    }
    // A binary midpoint's odd significand has at most 54 bits. Positive
    // decimal exponents above 23 introduce too many factors of five. Negative
    // exponents below -25 cannot cancel their fives with an 18-digit numerator.
    if !(-25..=23).contains(&exponent) {
        return None;
    }
    let fives = 5_u128.pow(exponent.unsigned_abs());
    if exponent >= 0 {
        numerator = numerator.checked_mul(fives)?;
    } else {
        if numerator % fives != 0 {
            return None;
        }
        numerator /= fives;
    }
    Some(normalize(numerator, exponent))
}

fn decimal_digits(mantissa: &str) -> Option<u128> {
    let mut digits = mantissa.bytes().filter(|byte| *byte != b'.').peekable();
    if digits.peek() == Some(&b'+') {
        digits.next();
    }
    let mut any = false;
    let mut value = 0_u128;
    for byte in digits {
        if !byte.is_ascii_digit() {
            return None;
        }
        any = true;
        value = value
            .checked_mul(10)?
            .checked_add(u128::from(byte - b'0'))?;
    }
    any.then_some(value)
}

fn binary_dyadic(bits: u64) -> (u128, i32) {
    let exponent = u16::try_from(bits >> 52).expect("positive finite exponent");
    let fraction = bits & ((1_u64 << 52) - 1);
    if exponent == 0 {
        (u128::from(fraction), -1074)
    } else {
        (
            u128::from(fraction | (1_u64 << 52)),
            i32::from(exponent) - 1075,
        )
    }
}

fn normalize(numerator: u128, exponent: i32) -> (u128, i32) {
    let zeros = numerator.trailing_zeros();
    (
        numerator >> zeros,
        exponent + i32::try_from(zeros).expect("at most 127 trailing zeros"),
    )
}

#[cfg(test)]
mod tests {
    use super::decimal_digits;

    #[test]
    fn should_match_previous_decimal_digit_parsing() {
        // Arrange
        let inputs = [
            "1.234",
            "+12.3",
            "-12.3",
            "1..2",
            ".",
            "",
            "12x",
            "0001",
            "340282366920938463463374607431768211456",
        ];
        // Act / Assert
        for input in inputs {
            // Assert
            assert_eq!(
                decimal_digits(input),
                input.replace('.', "").parse::<u128>().ok(),
                "{input}"
            );
        }
    }
}
