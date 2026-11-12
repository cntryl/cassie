//! Linear, bounded parsing of date/time format templates.

const KEYWORDS: &[&str] = &[
    "a.d.", "a.m.", "b.c.", "p.m.", "y,yyy", "hh24", "hh12", "iddd", "iyyy", "month", "sssss",
    "ssss", "yyyy", "day", "ddd", "ff1", "ff2", "ff3", "ff4", "ff5", "ff6", "iyy", "mon", "tzh",
    "tzm", "yyy", "ad", "am", "bc", "cc", "dd", "dy", "fx", "hh", "id", "iw", "iy", "mi", "mm",
    "ms", "of", "pm", "rm", "ss", "tz", "us", "ww", "yy", "d", "i", "j", "q", "w", "y",
];

const CHECK_INTERVAL: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Node {
    Literal(String),
    Field {
        keyword: &'static str,
        /// The template's own spelling, for letter-case output rules.
        spelling: String,
        fill: bool,
        ordinal: Option<bool>,
    },
}

pub(super) fn parse_with_check<E>(
    template: &str,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<Vec<Node>, E> {
    check()?;
    let mut chars = Vec::with_capacity(template.len());
    for (index, character) in template.chars().enumerate() {
        if index.is_multiple_of(CHECK_INTERVAL) {
            check()?;
        }
        chars.push(character);
    }
    let mut nodes = Vec::new();
    nodes.reserve_exact(chars.len());
    let mut index = 0;
    let mut fill = false;
    while index < chars.len() {
        if index.is_multiple_of(CHECK_INTERVAL) {
            check()?;
        }
        if starts_with_ascii_ignore_case(&chars, index, "fm") {
            fill = true;
            index += 2;
            continue;
        }
        if starts_with_ascii_ignore_case(&chars, index, "tm") {
            index += 2;
            continue;
        }
        if chars[index] == '"' {
            let mut literal = String::new();
            index += 1;
            while index < chars.len() && chars[index] != '"' {
                if index.is_multiple_of(CHECK_INTERVAL) {
                    check()?;
                }
                if chars[index] == '\\' && index + 1 < chars.len() {
                    index += 1;
                }
                literal.push(chars[index]);
                index += 1;
            }
            index += 1;
            nodes.push(Node::Literal(literal));
            continue;
        }
        if chars[index] == '\\' && index + 1 < chars.len() {
            nodes.push(Node::Literal(chars[index + 1].to_string()));
            index += 2;
            continue;
        }
        if let Some(keyword) = KEYWORDS
            .iter()
            .find(|keyword| starts_with_ascii_ignore_case(&chars, index, keyword))
        {
            let length = keyword.len();
            let spelling: String = chars[index..index + length].iter().collect();
            index += length;
            let ordinal = if chars.get(index) == Some(&'T') && chars.get(index + 1) == Some(&'H') {
                index += 2;
                Some(true)
            } else if chars.get(index) == Some(&'t') && chars.get(index + 1) == Some(&'h') {
                index += 2;
                Some(false)
            } else {
                None
            };
            nodes.push(Node::Field {
                keyword,
                spelling,
                fill,
                ordinal,
            });
            fill = false;
            continue;
        }
        nodes.push(Node::Literal(chars[index].to_string()));
        index += 1;
    }
    Ok(nodes)
}

pub(super) fn workspace_bytes(template_bytes: usize) -> Option<usize> {
    let nodes = std::mem::size_of::<Node>().checked_add(std::mem::size_of::<char>() + 1)?;
    template_bytes.checked_mul(nodes)
}

fn starts_with_ascii_ignore_case(chars: &[char], index: usize, keyword: &str) -> bool {
    keyword.bytes().enumerate().all(|(offset, byte)| {
        chars.get(index + offset).is_some_and(|character| {
            character.is_ascii() && character.to_ascii_lowercase() == char::from(byte)
        })
    })
}
