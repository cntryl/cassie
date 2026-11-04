//! `CHAR(n)` (`bpchar`) value semantics.
//!
//! Trailing blanks are insignificant in a blank-padded character value, so
//! `'x'` and `'x  '` are the same `CHAR(3)` value. Cassie stores the value
//! without its trailing blanks, which makes equality, UNIQUE reservations and
//! index keys agree on that one canonical spelling.

/// Returns `value` without trailing blanks: the canonical stored form of a
/// `CHAR(n)` value.
#[must_use]
pub fn canonical_char_text(value: &str) -> &str {
    value.trim_end_matches(' ')
}

/// Allocation-free character-counted typmod plan. Input and assignment only
/// discard excess ASCII spaces; explicit casts may discard any excess text.
#[derive(Debug)]
pub(crate) struct CharacterPlan {
    pub(crate) prefix_bytes: usize,
    pub(crate) padding: usize,
    pub(crate) output_bytes: usize,
}

pub(crate) fn conversion_plan(
    text: &str,
    width: Option<u32>,
    padded: bool,
    explicit: bool,
) -> Result<CharacterPlan, super::extended::TypeInputError> {
    let Some(width) = width else {
        return Ok(CharacterPlan {
            prefix_bytes: text.len(),
            padding: 0,
            output_bytes: text.len(),
        });
    };
    let width = usize::try_from(width).map_err(|_| {
        super::extended::TypeInputError::new(
            "54000",
            "character width exceeds platform range".into(),
        )
    })?;
    let prefix_bytes = text
        .char_indices()
        .nth(width)
        .map_or(text.len(), |(index, _)| index);
    if !explicit && text[prefix_bytes..].bytes().any(|byte| byte != b' ') {
        let kind = if padded {
            "character"
        } else {
            "character varying"
        };
        return Err(super::extended::TypeInputError::new(
            "22001",
            format!("value too long for type {kind}({width})"),
        ));
    }
    let padding = if padded {
        width.saturating_sub(text[..prefix_bytes].chars().count())
    } else {
        0
    };
    let output_bytes = prefix_bytes.checked_add(padding).ok_or_else(|| {
        super::extended::TypeInputError::new("54000", "character output size overflow".into())
    })?;
    Ok(CharacterPlan {
        prefix_bytes,
        padding,
        output_bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::{canonical_char_text, conversion_plan};

    #[test]
    fn should_drop_only_trailing_blanks() {
        // Arrange
        let values = ["x  ", "  x", "x", "   ", "x\t "];

        // Act
        let canonical = values.map(canonical_char_text);

        // Assert
        assert_eq!(canonical, ["x", "  x", "x", "", "x\t"]);
    }

    #[test]
    fn should_distinguish_explicit_character_truncation_from_input_overflow() {
        // Arrange
        let input = "é猫z";

        // Act
        let explicit = conversion_plan(input, Some(2), true, true).expect("explicit truncation");
        let invalid = conversion_plan(input, Some(2), true, false);
        let spaces = conversion_plan("é猫  ", Some(2), false, false).expect("excess ASCII spaces");
        let tab = conversion_plan("é猫\t", Some(2), false, false);

        // Assert
        assert_eq!(&input[..explicit.prefix_bytes], "é猫");
        assert_eq!(explicit.padding, 0);
        assert_eq!(invalid.expect_err("non-space overflow").sqlstate, "22001");
        assert_eq!(spaces.prefix_bytes, "é猫".len());
        assert_eq!(
            tab.expect_err("tab is not an ASCII space").sqlstate,
            "22001"
        );
    }
}
