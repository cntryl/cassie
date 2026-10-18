//! PostgreSQL's default FLOAT8 spelling, including strict rounding boundaries.
#[path = "float_text/dyadic.rs"]
mod dyadic;

pub(crate) fn format(value: f64) -> String {
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
    let decimal = shortest_decimal(value);
    let (mantissa, exponent) = decimal.split_once('e').expect("Rust scientific notation");
    let exponent = exponent.parse::<i32>().expect("Rust decimal exponent");
    let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
    if !(-4..15).contains(&exponent) {
        return format!("{mantissa}e{exponent:+03}");
    }
    let mut output = String::with_capacity(25);
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
        let point = usize::try_from(point).expect("positive decimal point");
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

fn shortest_decimal(value: f64) -> String {
    let shortest = format!("{value:e}");
    let (mantissa, _) = shortest.split_once('e').expect("Rust scientific notation");
    let precision = mantissa.bytes().filter(u8::is_ascii_digit).count() - 1;
    // The shortest formatter can round a decimal tie away from zero. Explicit
    // precision rounds ties to even. PostgreSQL also rejects binary midpoints,
    // even when a conforming parser would round them back to this value.
    for precision in precision..=17 {
        let candidate = format!("{value:.precision$e}");
        if candidate
            .parse::<f64>()
            .is_ok_and(|parsed| parsed.to_bits() == value.to_bits())
            && !dyadic::is_midpoint(value, &candidate)
        {
            return candidate;
        }
        // At a power of two the lower and upper rounding intervals differ.
        // The nearest decimal may lie outside the lower interval while the
        // shortest formatter selected another decimal at the same precision.
        if shortest
            .parse::<f64>()
            .is_ok_and(|parsed| parsed.to_bits() == value.to_bits())
            && !dyadic::is_midpoint(value, &shortest)
        {
            return shortest;
        }
    }
    // Eighteen significant decimal digits have a relative error smaller than
    // either binary midpoint for every finite, nonzero IEEE double.
    unreachable!("18 significant digits strictly roundtrip a finite double")
}
