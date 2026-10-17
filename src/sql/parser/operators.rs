use super::clauses::find_top_level_keyword_span;

pub(super) fn operator_character(character: char) -> bool {
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
        .find(|(_, character)| !character.is_ascii_digit() && !matches!(*character, '.' | '_'))
        .map_or(0, |(index, character)| index + character.len_utf8());
    let number = &mantissa[start..];
    let identifier_prefix = mantissa[..start]
        .chars()
        .next_back()
        .is_some_and(super::lexical::identifier_character);
    !identifier_prefix && super::expr::is_numeric_mantissa(number)
}

pub(super) fn numeric_exponent_prefix(input: &str) -> bool {
    let Some((mantissa, _)) = input.split_once(['e', 'E']) else {
        return false;
    };
    mantissa.starts_with(|character: char| {
        character.is_ascii_digit() || matches!(character, '+' | '-' | '.')
    }) && super::expr::is_numeric_mantissa(mantissa)
}

fn binary_span(input: &str, start: usize, end: usize, token: &str) -> bool {
    // These characters keep an adjacent trailing sign in the operator name
    // (for example PostgreSQL's undefined %-), rather than a unary operand.
    if matches!(token, "%" | "^" | "||" | "|" | "&" | "#") && input[end..].starts_with(['+', '-']) {
        return false;
    }
    let left = super::lexical::trim_separators(&input[..start]);
    if left.is_empty() || left.ends_with(operator_character) {
        return false;
    }
    if matches!(token, "+" | "-") && exponent_sign(input, start) {
        return false;
    }
    // A separated generic prefix can begin the right operand. Adjacent
    // punctuation belongs to one operator name, as with PostgreSQL's *|/.
    let tail = &input[end..];
    let operand = super::lexical::trim_separators(tail);
    let prefix = tail.len() != operand.len()
        && ["||/", "|/", "@", "~"]
            .iter()
            .any(|token| operand.starts_with(token));
    prefix
        || !operand.starts_with(|character: char| {
            operator_character(character) && !matches!(character, '+' | '-')
        })
}

pub(super) fn split_operator<'a>(
    input: &'a str,
    token: &str,
    last: bool,
) -> Option<(&'a str, &'a str)> {
    split_before(input, token, last, input.len())
}

pub(super) fn split_arithmetic<'a>(input: &'a str, token: &str) -> Option<(&'a str, &'a str)> {
    let mut before = input.len();
    for prefix in ["||/", "|/", "@", "~"] {
        let mut cursor = 0;
        while let Some((start, end)) = operator_span(input, cursor, prefix) {
            let left = super::lexical::trim_separators(&input[..start]);
            if left.is_empty() || left.ends_with(operator_character) {
                before = before.min(start);
                break;
            }
            cursor = end;
        }
    }
    split_before(input, token, true, before)
}

fn split_before<'a>(
    input: &'a str,
    token: &str,
    last: bool,
    before: usize,
) -> Option<(&'a str, &'a str)> {
    let mut cursor = 0;
    let mut selected = None;
    while let Some((start, end)) = operator_span(input, cursor, token) {
        if start < before && binary_span(input, start, end, token) {
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
        let unicode_prefix = token == "&"
            && start.checked_sub(1).is_some_and(|prefix| {
                matches!(input.as_bytes()[prefix], b'U' | b'u')
                    && input[end..].starts_with(['\'', '"'])
                    && input[..prefix]
                        .chars()
                        .next_back()
                        .is_none_or(|character| !super::lexical::identifier_character(character))
            });
        if !unicode_prefix && !input[start..].starts_with("--") && !input[start..].starts_with("/*")
        {
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
