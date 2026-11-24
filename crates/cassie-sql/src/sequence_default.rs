//! Pure parsing and canonicalization for sequence-backed default expressions.

/// Returns the canonical stored expression for a sequence default.
#[must_use]
pub fn canonical_nextval_expression(sequence: &str) -> String {
    format!("nextval('{sequence}'::regclass)")
}

/// Extracts a sequence name from a supported `nextval` default expression.
#[must_use]
pub fn parse_nextval_default_expression(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let lower = raw.to_ascii_lowercase();
    let inner = lower.strip_prefix("nextval(")?;
    if !inner.ends_with(')') {
        return None;
    }

    let original_inner = &raw["nextval(".len()..raw.len().saturating_sub(1)];
    let mut value = original_inner.trim();
    if let Some((candidate, cast)) = value.split_once("::") {
        if !cast.trim().eq_ignore_ascii_case("regclass") {
            return None;
        }
        value = candidate.trim();
    }

    if value.starts_with('\'') && value.ends_with('\'') && value.len() >= 2 {
        return Some(value[1..value.len() - 1].replace("''", "'"));
    }
    if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
        return Some(value[1..value.len() - 1].replace("\"\"", "\""));
    }
    if is_identifier(value) {
        return Some(value.to_string());
    }
    None
}

/// Derives the backing sequence name for a `SERIAL` column.
#[must_use]
pub fn serial_sequence_name(table: &str, field: &str) -> String {
    cassie_core::catalog::scope::derive_scoped_name(table, |local| format!("{local}_{field}_seq"))
}

fn is_identifier(raw: &str) -> bool {
    let mut chars = raw.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

#[cfg(test)]
mod tests {
    use super::{canonical_nextval_expression, parse_nextval_default_expression};

    #[test]
    fn should_parse_and_canonicalize_sequence_default_expressions() {
        // Arrange
        let expressions = [
            ("nextval('public.items_id_seq'::regclass)", "public.items_id_seq"),
            ("nextval(\"items_id_seq\")", "items_id_seq"),
        ];

        // Act
        let parsed = expressions.map(|(expression, _)| {
            parse_nextval_default_expression(expression).expect("recognized nextval")
        });

        // Assert
        assert_eq!(parsed[0], expressions[0].1);
        assert_eq!(parsed[1], expressions[1].1);
        assert_eq!(canonical_nextval_expression(&parsed[0]), expressions[0].0);
    }
}
