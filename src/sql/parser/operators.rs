use super::clauses::find_top_level_keyword_span;

fn operator_character(character: char) -> bool {
    matches!(
        character,
        '+' | '-'
            | '*'
            | '/'
            | '<'
            | '>'
            | '='
            | '!'
            | '~'
            | '@'
            | '#'
            | '%'
            | '^'
            | '&'
            | '|'
            | '?'
    )
}

fn exponent_sign(input: &str, position: usize) -> bool {
    let prefix = &input[..position];
    let Some(mantissa) = prefix.strip_suffix(['e', 'E']) else {
        return false;
    };
    let start = mantissa
        .char_indices()
        .rev()
        .find(|(_, character)| !character.is_ascii_digit() && *character != '.')
        .map_or(0, |(index, character)| index + character.len_utf8());
    let number = &mantissa[start..];
    let identifier_prefix = mantissa[..start]
        .chars()
        .next_back()
        .is_some_and(super::lexical::identifier_character);
    !identifier_prefix && !number.is_empty() && number.parse::<f64>().is_ok()
}

pub(super) fn numeric_exponent_prefix(input: &str) -> bool {
    let Some((mantissa, _)) = input.split_once(['e', 'E']) else {
        return false;
    };
    mantissa.starts_with(|character: char| {
        character.is_ascii_digit() || matches!(character, '+' | '-' | '.')
    }) && mantissa.parse::<f64>().is_ok()
}

fn binary_span(input: &str, start: usize, end: usize, token: &str) -> bool {
    let left = super::lexical::trim_separators(&input[..start]);
    if left.is_empty() || left.ends_with(operator_character) {
        return false;
    }
    if matches!(token, "+" | "-") && exponent_sign(input, start) {
        return false;
    }
    // A following sign belongs to a signed literal, not this operator token.
    !super::lexical::trim_separators(&input[end..]).starts_with(|character: char| {
        operator_character(character) && !matches!(character, '+' | '-')
    })
}

pub(super) fn split_operator<'a>(
    input: &'a str,
    token: &str,
    last: bool,
) -> Option<(&'a str, &'a str)> {
    let mut cursor = 0;
    let mut selected = None;
    while let Some((start, end)) = operator_span(input, cursor, token) {
        if binary_span(input, start, end, token) {
            selected = Some((&input[..start], &input[end..]));
            if !last {
                return selected;
            }
        }
        cursor = end;
    }
    selected
}

fn operator_span(input: &str, mut cursor: usize, token: &str) -> Option<(usize, usize)> {
    while let Some((start, end)) = find_top_level_keyword_span(input, cursor, token) {
        // Keyword patterns may intentionally start at comment separators;
        // punctuation tokens must never consume their opening delimiters.
        if !input[start..].starts_with("--") && !input[start..].starts_with("/*") {
            return Some((start, end));
        }
        cursor = end;
    }
    None
}

pub(super) fn contains_operator(input: &str) -> bool {
    [
        "+", "-", "*", "/", "<", ">", "=", "!", "~", "@", "#", "%", "^", "&", "|", "?",
    ]
    .into_iter()
    .any(|token| operator_span(input, 0, token).is_some())
}
