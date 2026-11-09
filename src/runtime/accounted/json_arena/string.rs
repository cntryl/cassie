//! On-demand Value-mode string decoding for the isolated checked cursor.
use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

use super::{Cursor, CursorError, Position};

#[cfg(test)]
#[path = "string_tests.rs"]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ErrorKind {
    ExpectedQuote,
    EofString,
    Control,
    InvalidEscape,
    LoneSurrogate,
    UnexpectedHexEnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::runtime::accounted::json_arena) struct Diagnostic {
    kind: ErrorKind,
    position: Position,
}

impl Diagnostic {
    pub(in crate::runtime::accounted::json_arena) fn write_message(
        &self,
        out: &mut impl std::fmt::Write,
    ) -> std::fmt::Result {
        let message = match self.kind {
            ErrorKind::ExpectedQuote => "expected `\"`",
            ErrorKind::EofString => "EOF while parsing a string",
            ErrorKind::Control => {
                "control character (\\u0000-\\u001F) found while parsing a string"
            }
            ErrorKind::InvalidEscape => "invalid escape",
            ErrorKind::LoneSurrogate => "lone leading surrogate in hex escape",
            ErrorKind::UnexpectedHexEnd => "unexpected end of hex escape",
        };
        write!(
            out,
            "{message} at line {} column {}",
            self.position.line, self.position.column
        )
    }
}

#[cfg(test)]
impl Diagnostic {
    pub(super) fn test_info(&self) -> (serde_json::error::Category, Position) {
        let category = if self.kind == ErrorKind::EofString {
            serde_json::error::Category::Eof
        } else {
            serde_json::error::Category::Syntax
        };
        (category, self.position)
    }

    pub(super) fn test_message(&self) -> String {
        let message = match self.kind {
            ErrorKind::ExpectedQuote => "expected `\"`",
            ErrorKind::EofString => "EOF while parsing a string",
            ErrorKind::Control => {
                "control character (\\u0000-\\u001F) found while parsing a string"
            }
            ErrorKind::InvalidEscape => "invalid escape",
            ErrorKind::LoneSurrogate => "lone leading surrogate in hex escape",
            ErrorKind::UnexpectedHexEnd => "unexpected end of hex escape",
        };
        format!(
            "{message} at line {} column {}",
            self.position.line, self.position.column
        )
    }
}

pub(in crate::runtime::accounted::json_arena) struct DecodedText {
    value: String,
    memory: QueryMemoryReservation,
}

impl DecodedText {
    pub(super) fn new(controls: &QueryExecutionControls) -> Result<Self, CursorError> {
        Ok(Self {
            value: String::new(),
            memory: controls
                .reserve_query_memory(0)
                .map_err(CursorError::Interrupted)?,
        })
    }

    pub(in crate::runtime::accounted::json_arena) fn as_str(&self) -> &str {
        &self.value
    }

    pub(in crate::runtime::accounted::json_arena) fn shares_tracker(
        &self,
        controls: &QueryExecutionControls,
    ) -> bool {
        self.memory.shares_tracker(controls)
    }

    pub(in crate::runtime::accounted::json_arena) fn into_parts(
        self,
    ) -> (String, QueryMemoryReservation) {
        (self.value, self.memory)
    }

    pub(super) fn push(
        &mut self,
        value: char,
        controls: &QueryExecutionControls,
        observer: &mut impl FnMut(),
    ) -> Result<(), CursorError> {
        controls
            .check_interruption()
            .map_err(CursorError::Interrupted)?;
        let needed = self
            .value
            .len()
            .checked_add(value.len_utf8())
            .ok_or_else(|| {
                CursorError::Interrupted(CassieError::ResourceLimit(
                    "JSON string capacity overflow".into(),
                ))
            })?;
        if needed > self.value.capacity() {
            let capacity = needed.max(4).checked_next_power_of_two().ok_or_else(|| {
                CursorError::Interrupted(CassieError::ResourceLimit(
                    "JSON string capacity overflow".into(),
                ))
            })?;
            let memory = self
                .memory
                .try_sibling(capacity)
                .map_err(CursorError::Interrupted)?;
            let mut replacement = String::new();
            replacement.try_reserve_exact(capacity).map_err(|error| {
                CursorError::Interrupted(crate::runtime::owned_memory::allocation_error(&error))
            })?;
            if replacement.capacity() > capacity {
                return Err(CursorError::Interrupted(CassieError::ResourceLimit(
                    "allocator exceeded admitted JSON string capacity".into(),
                )));
            }
            let mut start = 0;
            while start < self.value.len() {
                controls
                    .check_interruption()
                    .map_err(CursorError::Interrupted)?;
                let mut end = start.saturating_add(1024).min(self.value.len());
                while !self.value.is_char_boundary(end) {
                    end -= 1;
                }
                replacement.push_str(&self.value[start..end]);
                start = end;
                observer();
                controls
                    .check_interruption()
                    .map_err(CursorError::Interrupted)?;
            }
            let old = std::mem::replace(&mut self.value, replacement);
            let old_memory = std::mem::replace(&mut self.memory, memory);
            drop(old);
            drop(old_memory);
        }
        self.value.push(value);
        observer();
        controls
            .check_interruption()
            .map_err(CursorError::Interrupted)
    }
}

impl Cursor<'_> {
    pub(in crate::runtime::accounted::json_arena) fn read_string(
        &mut self,
    ) -> Result<DecodedText, CursorError> {
        self.read_string_with_observer(|| {})
    }

    #[cfg(test)]
    pub(in crate::runtime::accounted::json_arena) fn read_string_observed(
        &mut self,
        observer: impl FnMut(),
    ) -> Result<DecodedText, CursorError> {
        self.read_string_with_observer(observer)
    }

    fn read_string_with_observer(
        &mut self,
        mut observer: impl FnMut(),
    ) -> Result<DecodedText, CursorError> {
        self.controls
            .check_interruption()
            .map_err(CursorError::Interrupted)?;
        if self.peek() != Some(b'"') {
            return Err(self.syntax(ErrorKind::ExpectedQuote));
        }
        let _ = self.next()?;
        let mut text = DecodedText::new(&self.controls)?;
        loop {
            self.controls
                .check_interruption()
                .map_err(CursorError::Interrupted)?;
            let byte = self
                .peek()
                .ok_or_else(|| self.syntax(ErrorKind::EofString))?;
            match byte {
                b'"' => {
                    let _ = self.next()?;
                    return Ok(text);
                }
                b'\\' => {
                    let _ = self.next()?;
                    let value = self.read_escape()?;
                    text.push(value, &self.controls, &mut observer)?;
                }
                0..=0x1f => {
                    let _ = self.next()?;
                    return Err(self.syntax(ErrorKind::Control));
                }
                0x20..=0x7f => {
                    let _ = self.next()?;
                    text.push(char::from(byte), &self.controls, &mut observer)?;
                }
                _ => {
                    let width = match byte {
                        0xc2..=0xdf => 2,
                        0xe0..=0xef => 3,
                        0xf0..=0xf4 => 4,
                        _ => return Err(CursorError::InvalidUtf8),
                    };
                    let start = self.position.offset;
                    let end = start
                        .checked_add(width)
                        .ok_or_else(|| self.syntax(ErrorKind::EofString))?;
                    let bytes = self
                        .source
                        .get(start..end)
                        .ok_or(CursorError::InvalidUtf8)?;
                    let value = std::str::from_utf8(bytes)
                        .map_err(|_| CursorError::InvalidUtf8)?
                        .chars()
                        .next()
                        .ok_or(CursorError::InvalidUtf8)?;
                    for _ in 0..width {
                        let _ = self.next()?;
                    }
                    text.push(value, &self.controls, &mut observer)?;
                }
            }
        }
    }

    fn read_escape(&mut self) -> Result<char, CursorError> {
        match self.next_required()? {
            b'"' => Ok('"'),
            b'\\' => Ok('\\'),
            b'/' => Ok('/'),
            b'b' => Ok('\u{8}'),
            b'f' => Ok('\u{c}'),
            b'n' => Ok('\n'),
            b'r' => Ok('\r'),
            b't' => Ok('\t'),
            b'u' => self.read_unicode_escape(),
            _ => Err(self.syntax(ErrorKind::InvalidEscape)),
        }
    }

    fn read_unicode_escape(&mut self) -> Result<char, CursorError> {
        let first = self.read_hex()?;
        if (0xdc00..=0xdfff).contains(&first) {
            return Err(self.syntax(ErrorKind::LoneSurrogate));
        }
        if !(0xd800..=0xdbff).contains(&first) {
            return Ok(char::from_u32(u32::from(first)).expect("non-surrogate u16"));
        }
        if self.next_required()? != b'\\' || self.next_required()? != b'u' {
            return Err(self.syntax(ErrorKind::UnexpectedHexEnd));
        }
        let second = self.read_hex()?;
        if !(0xdc00..=0xdfff).contains(&second) {
            return Err(self.syntax(ErrorKind::LoneSurrogate));
        }
        let scalar = 0x1_0000 + ((u32::from(first) - 0xd800) << 10) + u32::from(second) - 0xdc00;
        Ok(char::from_u32(scalar).expect("surrogate pair"))
    }

    fn read_hex(&mut self) -> Result<u16, CursorError> {
        let mut digits = [0; 4];
        for digit in &mut digits {
            *digit = self.next_required()?;
        }
        let mut value = 0_u16;
        for digit in digits {
            let Some(hex) = char::from(digit).to_digit(16) else {
                return Err(self.syntax(ErrorKind::InvalidEscape));
            };
            value = (value << 4) | u16::try_from(hex).expect("hex digit");
        }
        Ok(value)
    }

    fn next_required(&mut self) -> Result<u8, CursorError> {
        self.next()?
            .ok_or_else(|| self.syntax(ErrorKind::EofString))
    }

    fn syntax(&self, kind: ErrorKind) -> CursorError {
        CursorError::Syntax(Diagnostic {
            kind,
            position: self.position(),
        })
    }
}
