use std::borrow::Cow;

fn identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$') || byte >= 128
}

fn identifier_character(character: char) -> bool {
    character.is_alphanumeric()
        || matches!(character, '_' | '$')
        || !character.is_ascii() && !character.is_whitespace()
}

fn word_boundary(input: &str, index: usize) -> bool {
    input[index..]
        .chars()
        .next()
        .is_none_or(|character| !identifier_character(character))
}

fn quoted_end(input: &str, start: usize) -> Option<usize> {
    let bytes = input.as_bytes();
    let quote = *bytes.get(start)?;
    if matches!(quote, b'\'' | b'"') {
        let mut index = start + 1;
        while index < bytes.len() {
            if bytes[index] == quote {
                if bytes.get(index + 1) == Some(&quote) {
                    index += 2;
                } else {
                    return Some(index + 1);
                }
            } else {
                index += 1;
            }
        }
        return Some(bytes.len());
    }
    if quote != b'$'
        || input[..start]
            .chars()
            .next_back()
            .is_some_and(identifier_character)
    {
        return None;
    }
    let mut end = start + 1;
    while bytes
        .get(end)
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        end += 1;
    }
    if bytes.get(end) != Some(&b'$') || end > start + 1 && bytes[start + 1].is_ascii_digit() {
        return None;
    }
    let delimiter = &input[start..=end];
    Some(
        input[end + 1..]
            .find(delimiter)
            .map_or(bytes.len(), |offset| end + 1 + offset + delimiter.len()),
    )
}

fn comment_end(input: &str, start: usize) -> Option<usize> {
    let bytes = input.as_bytes();
    if bytes.get(start..start + 2) == Some(b"--") {
        return Some(
            input[start..]
                .find(['\n', '\r'])
                .map_or(bytes.len(), |offset| start + offset),
        );
    }
    if bytes.get(start..start + 2) != Some(b"/*") {
        return None;
    }
    let mut depth = 1;
    let mut index = start + 2;
    while index < bytes.len() {
        match bytes.get(index..index + 2) {
            Some(b"/*") => {
                depth += 1;
                index += 2;
            }
            Some(b"*/") => {
                depth -= 1;
                index += 2;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => index += 1,
        }
    }
    Some(bytes.len())
}

pub(super) fn separator_end(input: &str, start: usize) -> usize {
    let mut index = start;
    loop {
        if let Some(end) = comment_end(input, index) {
            index = end;
        } else if let Some(character) = input
            .get(index..)
            .and_then(|tail| tail.chars().next())
            .filter(|character| character.is_whitespace())
        {
            index += character.len_utf8();
        } else {
            return index;
        }
    }
}

pub(super) fn trim_separators(input: &str) -> &str {
    let start = separator_end(input, 0);
    let mut index = start;
    let mut end = start;
    while index < input.len() {
        let skipped = separator_end(input, index);
        if skipped > index {
            index = skipped;
            continue;
        }
        index = quoted_end(input, index)
            .unwrap_or_else(|| index + input[index..].chars().next().map_or(0, char::len_utf8));
        end = index;
    }
    &input[start..end]
}

pub(super) fn first_separator(input: &str) -> Option<usize> {
    let mut index = 0;
    while index < input.len() {
        if let Some(end) = quoted_end(input, index) {
            index = end;
        } else if separator_end(input, index) > index {
            return Some(index);
        } else {
            index += input[index..].chars().next().map_or(0, char::len_utf8);
        }
    }
    None
}

pub(super) fn matching_paren(input: &str, open_at: usize) -> Option<usize> {
    if input.as_bytes().get(open_at) != Some(&b'(') {
        return None;
    }
    let mut depth = 1usize;
    let mut index = open_at + 1;
    while index < input.len() {
        if let Some(end) = quoted_end(input, index).or_else(|| comment_end(input, index)) {
            index = end;
            continue;
        }
        match input.as_bytes()[index] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
        index += input[index..].chars().next().map_or(0, char::len_utf8);
    }
    None
}

pub(super) fn without_comments(input: &str) -> Cow<'_, str> {
    let mut output: Option<Vec<u8>> = None;
    let mut index = 0;
    while index < input.len() {
        if let Some(end) = quoted_end(input, index) {
            index = end;
        } else if let Some(end) = comment_end(input, index) {
            output.get_or_insert_with(|| input.as_bytes().to_vec())[index..end].fill(b' ');
            index = end;
        } else {
            index += input[index..].chars().next().map_or(0, char::len_utf8);
        }
    }
    output.map_or(Cow::Borrowed(input), |bytes| {
        Cow::Owned(String::from_utf8(bytes).expect("comments replaced with ASCII spaces"))
    })
}

pub(super) fn pattern_end(input: &str, start: usize, pattern: &str) -> Option<usize> {
    let bytes = input.as_bytes();
    let trimmed = pattern.trim();
    let mut index = start;
    if pattern.starts_with(char::is_whitespace) {
        index = separator_end(input, index);
        if index == start {
            return None;
        }
    }
    let mut words = trimmed.split_whitespace().peekable();
    while let Some(word) = words.next() {
        if word
            .as_bytes()
            .first()
            .is_some_and(|byte| identifier_byte(*byte))
            && input[..index]
                .chars()
                .next_back()
                .is_some_and(identifier_character)
        {
            return None;
        }
        let end = index.checked_add(word.len())?;
        if !bytes.get(index..end)?.eq_ignore_ascii_case(word.as_bytes()) {
            return None;
        }
        if word
            .as_bytes()
            .last()
            .is_some_and(|byte| identifier_byte(*byte))
            && !word_boundary(input, end)
        {
            return None;
        }
        index = end;
        if words.peek().is_some() {
            let next = separator_end(input, index);
            if next == index {
                return None;
            }
            index = next;
        }
    }
    if pattern.ends_with(char::is_whitespace) {
        let end = separator_end(input, index);
        if end == index {
            return None;
        }
        index = end;
    }
    Some(index)
}

pub(super) fn find_top_level_span(
    input: &str,
    start: usize,
    pattern: &str,
) -> Option<(usize, usize)> {
    let mut index = 0;
    let mut depth = 0usize;
    let mut brackets = 0usize;
    let mut cases = 0usize;
    while index < input.len() {
        if let Some(end) = quoted_end(input, index) {
            index = end;
            continue;
        }
        if cases == 0 && depth == 0 && brackets == 0 && index >= start {
            if let Some(end) = pattern_end(input, index, pattern) {
                return Some((index, end));
            }
        }
        if let Some(end) = comment_end(input, index) {
            index = end;
            continue;
        }
        if pattern_end(input, index, "case").is_some() {
            cases += 1;
        } else if pattern_end(input, index, "end").is_some() {
            cases = cases.saturating_sub(1);
        }
        match input.as_bytes()[index] {
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            b'[' => brackets += 1,
            b']' => brackets = brackets.saturating_sub(1),
            _ => {}
        }
        index += input[index..].chars().next().map_or(0, char::len_utf8);
    }
    None
}

pub(super) fn strip_keyword_suffix<'a>(input: &'a str, keyword: &str) -> Option<&'a str> {
    let mut cursor = 0;
    while let Some((position, end)) = find_top_level_span(input, cursor, keyword) {
        if trim_separators(&input[end..]).is_empty() {
            return Some(trim_separators(&input[..position]));
        }
        cursor = end;
    }
    None
}
