use super::{dyadic, format};

fn prior_format(value: f64) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-Infinity"
        } else {
            "Infinity"
        }
        .into();
    }
    if value == 0.0 {
        return value.to_string();
    }
    let shortest = format!("{value:e}");
    let (mantissa, _) = shortest.split_once('e').expect("scientific");
    let precision = mantissa.bytes().filter(u8::is_ascii_digit).count() - 1;
    let decimal = (precision..=17)
        .find_map(|precision| {
            let candidate = format!("{value:.precision$e}");
            if candidate
                .parse::<f64>()
                .is_ok_and(|parsed| parsed.to_bits() == value.to_bits())
                && !dyadic::is_midpoint(value, &candidate)
            {
                return Some(candidate);
            }
            if shortest
                .parse::<f64>()
                .is_ok_and(|parsed| parsed.to_bits() == value.to_bits())
                && !dyadic::is_midpoint(value, &shortest)
            {
                return Some(shortest.clone());
            }
            None
        })
        .expect("roundtrip");
    let (mantissa, exponent) = decimal.split_once('e').expect("scientific");
    let exponent = exponent.parse::<i32>().expect("exponent");
    let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
    if !(-4..15).contains(&exponent) {
        return format!("{mantissa}e{exponent:+03}");
    }
    let mut output = String::new();
    let digits = if let Some(magnitude) = mantissa.strip_prefix('-') {
        output.push('-');
        magnitude
    } else {
        mantissa
    };
    let digits = digits.replace('.', "");
    let point = exponent + 1;
    if point <= 0 {
        output.push_str("0.");
        for _ in point..0 {
            output.push('0');
        }
        output.push_str(&digits);
    } else {
        let point = usize::try_from(point).expect("point");
        if point < digits.len() {
            output.push_str(&digits[..point]);
            output.push('.');
            output.push_str(&digits[point..]);
        } else {
            output.push_str(&digits);
            for _ in digits.len()..point {
                output.push('0');
            }
        }
    }
    output
}

#[test]
fn should_preserve_shared_float8_formatter_against_previous_algorithm() {
    // Arrange
    let cases = [
        0_u64,
        1,
        2,
        0x0010_0000_0000_0000,
        0x3fef_ffff_ffff_ffff,
        0x3ff0_0000_0000_0000,
        0x3ff0_0000_0000_0001,
        0x4340_0000_0000_0000,
        0x7fef_ffff_ffff_ffff,
        0x7ff0_0000_0000_0000,
        0x7ff8_0000_0000_0000,
    ];
    // Act / Assert
    for bits in cases {
        for sign in [0, 1_u64 << 63] {
            let value = f64::from_bits(bits | sign);
            // Assert
            assert_eq!(
                format(value),
                prior_format(value),
                "bits {:016x}",
                value.to_bits()
            );
        }
    }
}
