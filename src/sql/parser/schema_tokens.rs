//! Separates column declaration tokens outside quoted and parenthesized regions.
pub(in crate::sql::parser) fn tokenize_schema_field(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut depth = 0isize;
    let mut start = 0usize;

    for (idx, ch) in raw.char_indices() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '(' if !in_single && !in_double => depth += 1,
            ')' if !in_single && !in_double => depth -= 1,
            ' ' if !in_single && !in_double && depth == 0 => {
                if start != idx {
                    out.push(raw[start..idx].to_string());
                }
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }

    if start < raw.len() {
        out.push(raw[start..].to_string());
    }

    out.into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}
