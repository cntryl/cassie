//! Owned, interruptible flat PostgreSQL text-array operand; other shapes fall back.
use crate::app::CassieError;
use crate::executor::QueryError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

#[path = "json_delete_owned_dimension.rs"]
mod dimension;

#[cfg(test)]
#[path = "json_delete_owned_path_tests.rs"]
mod tests;

#[derive(Clone, Copy)]
enum Progress {
    Dimension,
    Scan,
    Copy,
    DirectoryMove,
}

pub(in crate::executor::filter) struct OwnedPath {
    values: Vec<Option<String>>,
    memory: QueryMemoryReservation,
}

struct Replacement {
    values: Vec<Option<String>>,
    memory: QueryMemoryReservation,
}

struct TextBuffer {
    text: String,
    memory: QueryMemoryReservation,
}

impl TextBuffer {
    fn new(controls: &QueryExecutionControls) -> Result<Self, QueryError> {
        Ok(Self {
            text: String::new(),
            memory: controls.reserve_query_memory(0)?,
        })
    }

    fn append(
        &mut self,
        text: &str,
        controls: &QueryExecutionControls,
        observe: &mut impl FnMut(Progress),
    ) -> Result<(), QueryError> {
        let needed = self
            .text
            .len()
            .checked_add(text.len())
            .ok_or_else(overflow)?;
        if needed > self.text.capacity() {
            let capacity = needed
                .max(8)
                .checked_next_power_of_two()
                .ok_or_else(overflow)?;
            let memory = controls.reserve_query_memory(capacity)?;
            let mut replacement = String::new();
            replacement
                .try_reserve_exact(capacity)
                .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
            if replacement.capacity() > capacity {
                return Err(overflow().into());
            }
            copy(&mut replacement, &self.text, controls, observe)?;
            let old = std::mem::replace(&mut self.text, replacement);
            let old_memory = std::mem::replace(&mut self.memory, memory);
            drop(old);
            drop(old_memory);
        }
        copy(&mut self.text, text, controls, observe)
    }
}

fn copy(
    target: &mut String,
    text: &str,
    controls: &QueryExecutionControls,
    observe: &mut impl FnMut(Progress),
) -> Result<(), QueryError> {
    let mut start = 0;
    while start < text.len() {
        controls.check_interruption()?;
        let mut end = start.saturating_add(1024).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        target.push_str(&text[start..end]);
        start = end;
        observe(Progress::Copy);
        controls.check_interruption()?;
    }
    Ok(())
}

impl OwnedPath {
    fn new(controls: &QueryExecutionControls) -> Result<Self, QueryError> {
        Ok(Self {
            values: Vec::new(),
            memory: controls.reserve_query_memory(0)?,
        })
    }

    pub(in crate::executor::filter) fn values(&self) -> &[Option<String>] {
        &self.values
    }

    fn push(
        &mut self,
        item: Option<TextBuffer>,
        controls: &QueryExecutionControls,
        observe: &mut impl FnMut(Progress),
    ) -> Result<(), QueryError> {
        if self.values.len() == self.values.capacity() {
            let capacity = self
                .values
                .len()
                .checked_mul(2)
                .unwrap_or(usize::MAX)
                .max(4);
            let bytes = capacity
                .checked_mul(std::mem::size_of::<Option<String>>())
                .ok_or_else(overflow)?;
            let mut replacement = Replacement {
                values: Vec::new(),
                memory: controls.reserve_query_memory(bytes)?,
            };
            replacement
                .values
                .try_reserve_exact(capacity)
                .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
            if replacement.values.capacity() > capacity {
                return Err(overflow().into());
            }
            while let Some(value) = self.values.pop() {
                controls.check_interruption()?;
                replacement.values.push(value);
                observe(Progress::DirectoryMove);
                controls.check_interruption()?;
            }
            let mut left = 0;
            let mut right = replacement.values.len();
            while left < right {
                controls.check_interruption()?;
                right -= 1;
                if left >= right {
                    break;
                }
                replacement.values.swap(left, right);
                left += 1;
            }
            let old_bytes = self
                .values
                .capacity()
                .checked_mul(std::mem::size_of::<Option<String>>())
                .ok_or_else(overflow)?;
            let Replacement { values, memory } = replacement;
            let old = std::mem::replace(&mut self.values, values);
            drop(old);
            self.memory.shrink_to(self.memory.bytes() - old_bytes);
            self.memory.absorb(memory);
        }
        match item {
            Some(TextBuffer { text, memory }) => {
                self.memory.absorb(memory);
                self.values.push(Some(text));
            }
            None => self.values.push(None),
        }
        Ok(())
    }
}

fn overflow() -> CassieError {
    CassieError::ResourceLimit("text-array operand capacity overflow".into())
}
fn syntax(message: &str) -> QueryError {
    CassieError::ArrayInput(message.into()).into()
}

struct Cursor<'a, F> {
    characters: std::iter::Peekable<std::str::Chars<'a>>,
    controls: &'a QueryExecutionControls,
    observe: F,
}

impl<F: FnMut(Progress)> Cursor<'_, F> {
    fn peek(&mut self) -> Option<char> {
        self.characters.peek().copied()
    }
    fn next(&mut self) -> Result<Option<char>, QueryError> {
        self.controls.check_interruption()?;
        let result = self.characters.next();
        if result.is_some() {
            (self.observe)(Progress::Scan);
            self.controls.check_interruption()?;
        }
        Ok(result)
    }
    fn whitespace(&mut self) -> Result<(), QueryError> {
        while self.peek().is_some_and(char::is_whitespace) {
            let _consumed = self.next()?;
        }
        Ok(())
    }
    fn append(&mut self, buffer: &mut TextBuffer, text: &str) -> Result<(), QueryError> {
        buffer.append(text, self.controls, &mut self.observe)
    }
    fn quoted(&mut self) -> Result<TextBuffer, QueryError> {
        let _consumed = self.next()?;
        let mut output = TextBuffer::new(self.controls)?;
        let mut encoded = [0; 4];
        loop {
            match self.next()? {
                Some('"') => return Ok(output),
                Some('\\') => {
                    let value = self
                        .next()?
                        .ok_or_else(|| syntax("unterminated escape in PostgreSQL array"))?;
                    self.append(&mut output, value.encode_utf8(&mut encoded))?;
                }
                Some(value) => self.append(&mut output, value.encode_utf8(&mut encoded))?,
                None => return Err(syntax("unterminated quoted PostgreSQL array element")),
            }
        }
    }
    fn unquoted(&mut self) -> Result<TextBuffer, QueryError> {
        let mut output = TextBuffer::new(self.controls)?;
        let mut trailing = TextBuffer::new(self.controls)?;
        let mut encoded = [0; 4];
        loop {
            match self.peek() {
                Some(',' | '}') => break,
                Some('"' | '{') => {
                    return Err(syntax(
                        "invalid character in unquoted PostgreSQL array element",
                    ))
                }
                Some('\\') => {
                    let _consumed = self.next()?;
                    self.append(&mut output, &trailing.text)?;
                    trailing.text.clear();
                    let value = self
                        .next()?
                        .ok_or_else(|| syntax("unterminated escape in PostgreSQL array"))?;
                    self.append(&mut output, value.encode_utf8(&mut encoded))?;
                }
                Some(value) if value.is_whitespace() => {
                    let _consumed = self.next()?;
                    self.append(&mut trailing, value.encode_utf8(&mut encoded))?;
                }
                Some(value) => {
                    let _consumed = self.next()?;
                    self.append(&mut output, &trailing.text)?;
                    trailing.text.clear();
                    self.append(&mut output, value.encode_utf8(&mut encoded))?;
                }
                None => return Err(syntax("unterminated PostgreSQL array literal")),
            }
        }
        if output.text.is_empty() {
            Err(syntax("empty unquoted PostgreSQL array element"))
        } else {
            Ok(output)
        }
    }
}

pub(in crate::executor::filter) fn parse(
    input: &str,
    controls: &QueryExecutionControls,
) -> Result<Option<OwnedPath>, QueryError> {
    parse_observing(input, controls, |_| {})
}

fn parse_observing(
    input: &str,
    controls: &QueryExecutionControls,
    mut observe: impl FnMut(Progress),
) -> Result<Option<OwnedPath>, QueryError> {
    controls.check_interruption()?;
    if let Some((length, body)) = dimension::body(input, controls, &mut observe)? {
        return match parse_flat_observing(body, controls, &mut observe) {
            Ok(Some(path)) if path.values.is_empty() || path.values.len() == length => {
                Ok(Some(path))
            }
            Ok(_) | Err(QueryError::Cassie(CassieError::ArrayInput(_))) => Ok(None),
            Err(error) => Err(error),
        };
    }
    parse_flat_observing(input, controls, observe)
}

fn parse_flat_observing(
    input: &str,
    controls: &QueryExecutionControls,
    mut observe: impl FnMut(Progress),
) -> Result<Option<OwnedPath>, QueryError> {
    controls.check_interruption()?;
    let mut probe = input.chars().peekable();
    while probe.peek().is_some_and(|ch| ch.is_whitespace()) {
        controls.check_interruption()?;
        let _consumed = probe.next();
    }
    if probe.next() != Some('{') {
        return Ok(None);
    }
    let mut quoted = false;
    let mut escaped = false;
    for ch in probe {
        controls.check_interruption()?;
        observe(Progress::Scan);
        controls.check_interruption()?;
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            quoted = !quoted;
            continue;
        }
        if ch == '{' && !quoted {
            return Ok(None);
        }
    }
    let mut cursor = Cursor {
        characters: input.chars().peekable(),
        controls,
        observe,
    };
    let mut output = OwnedPath::new(controls)?;
    cursor.whitespace()?;
    let _consumed = cursor.next()?;
    cursor.whitespace()?;
    if cursor.peek() == Some('}') {
        let _consumed = cursor.next()?;
    } else {
        loop {
            let quoted = cursor.peek() == Some('"');
            let value = if quoted {
                cursor.quoted()?
            } else {
                cursor.unquoted()?
            };
            let is_null = !quoted && value.text.eq_ignore_ascii_case("NULL");
            output.push(
                if is_null { None } else { Some(value) },
                controls,
                &mut cursor.observe,
            )?;
            cursor.whitespace()?;
            match cursor.next()? {
                Some(',') => {
                    cursor.whitespace()?;
                    if matches!(cursor.peek(), None | Some('}')) {
                        return Err(syntax("empty PostgreSQL array element"));
                    }
                }
                Some('}') => break,
                _ => return Err(syntax("invalid PostgreSQL array delimiter")),
            }
        }
    }
    cursor.whitespace()?;
    if cursor.next()?.is_some() {
        return Err(syntax("trailing data after PostgreSQL array literal"));
    }
    i32::try_from(output.values.len()).map_err(|_| syntax("array dimension exceeds INT4"))?;
    Ok(Some(output))
}
