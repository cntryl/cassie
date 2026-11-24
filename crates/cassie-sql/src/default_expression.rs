//! Pure recognition and canonical spelling for supported volatile defaults.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolatileDefault {
    Now,
    CurrentTimestamp,
    LocalTimestamp,
    CurrentDate,
    RandomUuid,
}

impl VolatileDefault {
    pub fn parse(raw: &str) -> Option<Self> {
        let normalized = raw
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect::<String>()
            .to_ascii_lowercase();
        // `DEFAULT (now())` names the same function; only exact names match
        // below, so peeling outer parentheses cannot admit an expression.
        let mut unwrapped = normalized.as_str();
        while let Some(inner) = unwrapped
            .strip_prefix('(')
            .and_then(|rest| rest.strip_suffix(')'))
        {
            unwrapped = inner;
        }
        let without_precision = strip_precision(unwrapped);
        match without_precision {
            "now()" => Some(Self::Now),
            "current_timestamp" => Some(Self::CurrentTimestamp),
            "localtimestamp" => Some(Self::LocalTimestamp),
            "current_date" => Some(Self::CurrentDate),
            "gen_random_uuid()" => Some(Self::RandomUuid),
            _ => None,
        }
    }

    pub const fn canonical(self) -> &'static str {
        match self {
            Self::Now => "now()",
            Self::CurrentTimestamp => "CURRENT_TIMESTAMP",
            Self::LocalTimestamp => "LOCALTIMESTAMP",
            Self::CurrentDate => "CURRENT_DATE",
            Self::RandomUuid => "gen_random_uuid()",
        }
    }
}

/// Drops a `(precision)` suffix from `current_timestamp(3)` or
/// `localtimestamp(3)`; any other text is returned unchanged.
fn strip_precision(normalized: &str) -> &str {
    let Some(open) = normalized.find('(') else {
        return normalized;
    };
    let (name, rest) = normalized.split_at(open);
    let digits = rest
        .strip_prefix('(')
        .and_then(|rest| rest.strip_suffix(')'))
        .unwrap_or_default();
    if matches!(name, "current_timestamp" | "localtimestamp")
        && !digits.is_empty()
        && digits.chars().all(|character| character.is_ascii_digit())
    {
        name
    } else {
        normalized
    }
}

/// Returns the canonical stored expression for a supported volatile column default.
#[must_use]
pub fn parse_volatile_default_expression(raw: &str) -> Option<String> {
    VolatileDefault::parse(raw).map(|default| default.canonical().to_string())
}

#[cfg(test)]
mod tests {
    use super::parse_volatile_default_expression;

    #[test]
    fn should_recognize_parenthesized_volatile_default() {
        // Arrange
        let spellings = ["(now())", "((CURRENT_DATE))", "(now()) + (now())"];

        // Act
        let canonical = spellings
            .iter()
            .map(|spelling| parse_volatile_default_expression(spelling))
            .collect::<Vec<_>>();

        // Assert
        assert_eq!(
            canonical,
            vec![Some("now()".to_string()), Some("CURRENT_DATE".to_string()), None]
        );
    }
}
