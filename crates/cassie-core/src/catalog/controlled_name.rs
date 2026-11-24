//! Admitted catalog identifier matching shared by the engine and storage.

use crate::request::text::{allocation_error, copy_text, grow_text_vec, TextError, TextRequest};
use crate::request::RequestMemoryReservation;

pub struct Parts {
    pub values: Vec<String>,
    memory: RequestMemoryReservation,
}

struct NameText {
    text: String,
    memory: RequestMemoryReservation,
}

impl NameText {
    fn new(controls: &TextRequest<'_>) -> Result<Self, TextError> {
        Ok(Self {
            text: String::new(),
            memory: controls.reserve(0)?,
        })
    }

    fn push(
        &mut self,
        character: char,
        controls: &TextRequest<'_>,
    ) -> Result<(), TextError> {
        controls.check_interruption()?;
        let needed = self
            .text
            .len()
            .checked_add(character.len_utf8())
            .ok_or_else(|| TextError::ResourceLimit("catalog name capacity overflow".into()))?;
        if needed > self.text.capacity() {
            let capacity = needed.max(4).checked_next_power_of_two().ok_or_else(|| {
                TextError::ResourceLimit("catalog name capacity overflow".into())
            })?;
            let replacement_memory = controls.reserve(capacity)?;
            let mut replacement = String::new();
            replacement
                .try_reserve_exact(capacity)
                .map_err(allocation_error)?;
            if replacement.capacity() > capacity {
                return Err(TextError::ResourceLimit(
                    "allocator exceeded admitted catalog name capacity".into(),
                ));
            }
            let mut start = 0;
            while start < self.text.len() {
                controls.check_interruption()?;
                let mut end = (start + 4096).min(self.text.len());
                while !self.text.is_char_boundary(end) {
                    end -= 1;
                }
                replacement.push_str(&self.text[start..end]);
                start = end;
            }
            let old = std::mem::replace(&mut self.text, replacement);
            drop(old);
            self.memory.shrink_to(0);
            self.memory.absorb(replacement_memory);
        }
        self.text.push(character);
        Ok(())
    }
}

/// Trims a catalog name while polling the request.
///
/// # Errors
/// Returns an interruption or admission error.
pub fn trim_checked<'a>(
    raw: &'a str,
    controls: &TextRequest<'_>,
) -> Result<&'a str, TextError> {
    let mut first = None;
    let mut last = 0;
    let mut next_check = 0;
    for (offset, character) in raw.char_indices() {
        if offset >= next_check {
            controls.check_interruption()?;
            next_check = offset.saturating_add(4096);
        }
        if !character.is_whitespace() {
            first.get_or_insert(offset);
            last = offset + character.len_utf8();
        }
    }
    Ok(first.map_or("", |start| &raw[start..last]))
}

fn push_part(
    parts: &mut Parts,
    text: &str,
    controls: &TextRequest<'_>,
) -> Result<(), TextError> {
    grow_text_vec(&mut parts.values, &mut parts.memory, controls)?;
    let (text, memory) = copy_text(text, controls)?;
    parts.values.push(text);
    parts.memory.absorb(memory);
    Ok(())
}

/// Parses identifier components and retains their admitted allocations.
///
/// # Errors
/// Returns an interruption or admission error.
pub fn parse_parts(
    raw: &str,
    controls: &TextRequest<'_>,
) -> Result<Option<Parts>, TextError> {
    controls.check_interruption()?;
    let raw = trim_checked(raw, controls)?;
    let mut parts = Parts {
        values: Vec::new(),
        memory: controls.reserve(0)?,
    };
    let mut current = NameText::new(controls)?;
    let mut chars = raw.char_indices().peekable();
    let mut quoted = false;
    let mut in_quotes = false;
    let mut closed_quote = false;
    let mut next_check = 0;
    while let Some((offset, character)) = chars.next() {
        if offset >= next_check {
            controls.check_interruption()?;
            next_check = offset.saturating_add(4096);
        }
        if in_quotes {
            if character == '"' {
                if chars.peek().is_some_and(|(_, next)| *next == '"') {
                    current.push('"', controls)?;
                    chars.next();
                } else {
                    in_quotes = false;
                    closed_quote = true;
                }
            } else {
                current.push(character, controls)?;
            }
            continue;
        }
        match character {
            '"' => {
                if closed_quote || !trim_checked(&current.text, controls)?.is_empty() {
                    return Ok(None);
                }
                current.text.clear();
                quoted = true;
                in_quotes = true;
            }
            '.' => {
                let part = if quoted {
                    current.text.as_str()
                } else {
                    trim_checked(&current.text, controls)?
                };
                if part.is_empty() {
                    return Ok(None);
                }
                push_folded_part(&mut parts, part, quoted, controls)?;
                current.text.clear();
                quoted = false;
                closed_quote = false;
            }
            character if closed_quote && character.is_whitespace() => {}
            _ if closed_quote => return Ok(None),
            _ => current.push(character, controls)?,
        }
    }
    if in_quotes {
        return Ok(None);
    }
    let part = if quoted {
        current.text.as_str()
    } else {
        trim_checked(&current.text, controls)?
    };
    if part.is_empty() {
        return Ok(None);
    }
    push_folded_part(&mut parts, part, quoted, controls)?;
    Ok(Some(parts))
}

/// An unquoted identifier folds to lower case; a quoted one keeps its case.
fn push_folded_part(
    parts: &mut Parts,
    part: &str,
    quoted: bool,
    controls: &TextRequest<'_>,
) -> Result<(), TextError> {
    if quoted || !part.bytes().any(|byte| byte.is_ascii_uppercase()) {
        return push_part(parts, part, controls);
    }
    push_part(parts, &part.to_ascii_lowercase(), controls)
}

fn exactly_equal(
    left: &str,
    right: &str,
    controls: &TextRequest<'_>,
) -> Result<bool, TextError> {
    if left.len() != right.len() {
        return Ok(false);
    }
    for (left, right) in left
        .as_bytes()
        .chunks(4096)
        .zip(right.as_bytes().chunks(4096))
    {
        controls.check_interruption()?;
        if left != right {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Matches stored and requested identifiers using catalog suffix and case rules.
///
/// # Errors
/// Returns an interruption or admission error.
pub fn name_matches_controlled(
    stored: &str,
    requested: &str,
    controls: &TextRequest<'_>,
) -> Result<bool, TextError> {
    controls.check_interruption()?;
    if exactly_equal(stored, requested, controls)? {
        return Ok(true);
    }
    let Some(stored) = parse_parts(stored, controls)? else {
        return Ok(false);
    };
    let Some(requested) = parse_parts(requested, controls)? else {
        return Ok(false);
    };
    if requested.values.len() > stored.values.len() {
        return Ok(false);
    }
    for (left, right) in stored
        .values
        .iter()
        .rev()
        .zip(requested.values.iter().rev())
    {
        controls.check_interruption()?;
        if !exactly_equal(left, right, controls)? {
            return Ok(false);
        }
    }
    Ok(true)
}
