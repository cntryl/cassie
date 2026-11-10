//! PostgreSQL 18's case-sensitive interval `to_char` token validation.

// In PostgreSQL's DCH_keywords order: prefix matches prefer the first entry.
const KEYWORDS: &[&str] = &[
    "A.D.", "A.M.", "AD", "AM", "B.C.", "BC", "CC", "DAY", "DDD", "DD", "DY", "Day", "Dy", "D",
    "FF1", "FF2", "FF3", "FF4", "FF5", "FF6", "FX", "HH24", "HH12", "HH", "IDDD", "ID", "IW",
    "IYYY", "IYY", "IY", "I", "J", "MI", "MM", "MONTH", "MON", "MS", "Month", "Mon", "OF", "P.M.",
    "PM", "Q", "RM", "SSSSS", "SSSS", "SS", "TZH", "TZM", "TZ", "US", "WW", "W", "Y,YYY", "YYYY",
    "YYY", "YY", "Y", "a.d.", "a.m.", "ad", "am", "b.c.", "bc", "cc", "day", "ddd", "dd", "dy",
    "d", "ff1", "ff2", "ff3", "ff4", "ff5", "ff6", "fx", "hh24", "hh12", "hh", "iddd", "id", "iw",
    "iyyy", "iyy", "iy", "i", "j", "mi", "mm", "month", "mon", "ms", "of", "p.m.", "pm", "q", "rm",
    "sssss", "ssss", "ss", "tzh", "tzm", "tz", "us", "ww", "w", "y,yyy", "yyyy", "yyy", "yy", "y",
];

fn invalid_keyword(keyword: &str) -> bool {
    [
        "tz", "tzh", "tzm", "of", "ad", "bc", "a.d.", "b.c.", "month", "mon", "day", "dy", "d",
        "id",
    ]
    .iter()
    .any(|invalid| keyword.eq_ignore_ascii_case(invalid))
}

/// Returns whether a parsed PostgreSQL action is forbidden for intervals.
pub(super) fn invalid<E>(
    template: &str,
    mut check: impl FnMut() -> Result<(), E>,
) -> Result<bool, E> {
    let bytes = template.as_bytes();
    let mut position = 0;
    let mut steps = 0;
    let mut quoted = false;
    check()?;
    while position < bytes.len() {
        steps += 1;
        if steps == 1024 {
            check()?;
            steps = 0;
        }
        if quoted {
            if bytes[position] == b'\\' && position + 1 < bytes.len() {
                position += 2;
            } else if bytes[position] == b'"' {
                quoted = false;
                position += 1;
            } else {
                position += 1;
            }
            continue;
        }
        let rest = &bytes[position..];
        if [b"FM", b"fm", b"TM", b"tm"]
            .iter()
            .any(|prefix| rest.starts_with(*prefix))
        {
            position += 2;
        }
        let rest = &bytes[position..];
        if let Some(keyword) = KEYWORDS
            .iter()
            .find(|keyword| rest.starts_with(keyword.as_bytes()))
        {
            if invalid_keyword(keyword) {
                check()?;
                return Ok(true);
            }
            position += keyword.len();
            let rest = &bytes[position..];
            if [b"TH", b"th", b"SP"]
                .iter()
                .any(|suffix| rest.starts_with(*suffix))
            {
                position += 2;
            }
        } else if position < bytes.len() {
            if bytes[position] == b'"' {
                quoted = true;
                position += 1;
            } else if bytes[position] == b'\\' && bytes.get(position + 1) == Some(&b'"') {
                position += 2;
            } else {
                position += 1;
            }
        }
    }
    check()?;
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::{invalid, invalid_keyword, KEYWORDS};

    #[test]
    fn should_classify_every_pinned_keyword_with_consume_allowed_keywords() {
        // Arrange
        let check = || Ok::<(), ()>(());

        // Act and Assert
        for keyword in KEYWORDS {
            // Assert
            assert_eq!(
                invalid(keyword, check),
                Ok(invalid_keyword(keyword)),
                "{keyword}"
            );
            assert_eq!(
                invalid(&format!("{keyword}|DAY"), check),
                Ok(true),
                "{keyword}"
            );
        }
    }

    #[test]
    fn should_distinguish_case_literals_modifiers_quotes_with_backslashes() {
        // Arrange
        let cases = [
            ("tZh", false),
            ("mOnTh", false),
            ("dAy", true),
            ("\"DAY\"", false),
            ("FM\"DAY\"", false),
            ("\\DAY", true),
            ("\\\"DAY", true),
            ("TMTZ", true),
            ("DDDth", false),
            ("é\"DAY\"x", false),
            ("é\"DAY\"d", true),
            ("DDDAY", false),
        ];

        for (template, expected) in cases {
            // Act
            let actual = invalid(template, || Ok::<(), ()>(()));

            // Assert
            assert_eq!(actual, Ok(expected), "{template}");
        }
    }

    #[test]
    fn should_interrupt_during_a_long_quoted_literal() {
        // Arrange
        let template = format!("\"{}\"", "x".repeat(2_048));
        let mut calls = 0;

        // Act
        let result = invalid(&template, || {
            calls += 1;
            if calls == 2 {
                Err("cancelled")
            } else {
                Ok(())
            }
        });

        // Assert
        assert_eq!(result, Err("cancelled"));
        assert_eq!(calls, 2);
    }
}
