//! `to_number(text, text)`: reads a number through a numeric template.
use super::QueryError;
use crate::types::extended::numeric::Numeric;
use crate::types::extended::TypeInputError;

fn invalid(message: String) -> QueryError {
    super::temporal::type_error(TypeInputError::new("22P02", message))
}

/// Roman numerals (`RN`) up to 3999.
fn roman(input: &str) -> Result<i64, QueryError> {
    let text = input.trim().to_ascii_uppercase();
    let value = |c: char| match c {
        'I' => Some(1),
        'V' => Some(5),
        'X' => Some(10),
        'L' => Some(50),
        'C' => Some(100),
        'D' => Some(500),
        'M' => Some(1000),
        _ => None,
    };
    let digits = text
        .chars()
        .map(value)
        .collect::<Option<Vec<i64>>>()
        .filter(|digits| !digits.is_empty())
        .ok_or_else(|| invalid(format!("invalid Roman numeral \"{input}\"")))?;
    let mut total = 0;
    for (index, digit) in digits.iter().enumerate() {
        if digits.get(index + 1).is_some_and(|next| next > digit) {
            total -= digit;
        } else {
            total += digit;
        }
    }
    if !(1..=3999).contains(&total) {
        return Err(invalid(format!("invalid Roman numeral \"{input}\"")));
    }
    Ok(total)
}

/// `to_number`: digits, one decimal point (`.`/`D` in the template) and a
/// sign (`-`, or `(...)` with `PR`) are read; grouping and fill characters
/// are skipped.
pub(super) fn to_number(input: &str, template: &str) -> Result<Numeric, QueryError> {
    let upper = template.to_ascii_uppercase();
    if upper.contains("RN") {
        return roman(input).map(Numeric::from_i64);
    }
    let decimal = upper.contains('.') || upper.contains('D');
    let mut digits = String::new();
    let mut fraction = None::<usize>;
    let mut negative = false;
    for c in input.chars() {
        match c {
            '0'..='9' => digits.push(c),
            '.' if decimal && fraction.is_none() => fraction = Some(digits.len()),
            '-' => negative = true,
            '(' if upper.contains("PR") => negative = true,
            _ => {}
        }
    }
    if digits.is_empty() {
        return Err(invalid(format!(
            "invalid input syntax for type numeric: \"{input}\""
        )));
    }
    let scale = fraction.map_or(0, |position| digits.len() - position);
    let mantissa = digits.parse::<num_bigint::BigInt>().map_err(|_| {
        invalid(format!(
            "invalid input syntax for type numeric: \"{input}\""
        ))
    })?;
    let mantissa = if negative { -mantissa } else { mantissa };
    Ok(Numeric::finite(
        mantissa,
        u32::try_from(scale).unwrap_or(u32::MAX),
    ))
}

#[cfg(test)]
mod tests {
    use super::to_number;

    #[test]
    fn should_read_numbers_through_templates_like_postgresql() {
        // Arrange
        let cases = [
            ("XII", "RN", "12"),
            ("12,454.8-", "99G999D9S", "-12454.8"),
            ("  -  1 2 . 5", "9999.9", "-12.5"),
        ];

        // Act
        let parsed = cases.map(|(input, template, _)| {
            to_number(input, template)
                .map(|value| value.to_string())
                .ok()
        });

        // Assert
        for (index, (_, _, expected)) in cases.iter().enumerate() {
            assert_eq!(parsed[index].as_deref(), Some(*expected));
        }
    }
}
