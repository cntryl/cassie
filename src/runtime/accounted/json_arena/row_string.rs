//! Test-only SliceRead-style deferred UTF-8 validation for row strings.
use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

use super::{Cursor, CursorError, DecodedText, Diagnostic, ErrorKind};

struct ByteBuffer {
    bytes: Vec<u8>,
    memory: QueryMemoryReservation,
}

impl ByteBuffer {
    fn new(controls: &QueryExecutionControls) -> Result<Self, CursorError> {
        Ok(Self {
            bytes: Vec::new(),
            memory: controls
                .reserve_query_memory(0)
                .map_err(CursorError::Interrupted)?,
        })
    }

    fn push(
        &mut self,
        value: u8,
        controls: &QueryExecutionControls,
        observe: &mut impl FnMut(),
    ) -> Result<(), CursorError> {
        controls
            .check_interruption()
            .map_err(CursorError::Interrupted)?;
        if self.bytes.len() == self.bytes.capacity() {
            let capacity = self
                .bytes
                .len()
                .max(4)
                .checked_mul(2)
                .ok_or_else(overflow)?;
            let mut replacement = Self {
                bytes: Vec::new(),
                memory: self
                    .memory
                    .try_sibling(capacity)
                    .map_err(CursorError::Interrupted)?,
            };
            replacement
                .bytes
                .try_reserve_exact(capacity)
                .map_err(|error| {
                    CursorError::Interrupted(crate::runtime::owned_memory::allocation_error(&error))
                })?;
            if replacement.bytes.capacity() > capacity {
                return Err(overflow());
            }
            for chunk in self.bytes.chunks(1024) {
                controls
                    .check_interruption()
                    .map_err(CursorError::Interrupted)?;
                replacement.bytes.extend_from_slice(chunk);
                observe();
                controls
                    .check_interruption()
                    .map_err(CursorError::Interrupted)?;
            }
            let old_bytes = std::mem::replace(&mut self.bytes, replacement.bytes);
            let old_memory = std::mem::replace(&mut self.memory, replacement.memory);
            drop(old_bytes);
            drop(old_memory);
        }
        self.bytes.push(value);
        observe();
        controls
            .check_interruption()
            .map_err(CursorError::Interrupted)
    }
}

fn overflow() -> CursorError {
    CursorError::Interrupted(CassieError::ResourceLimit(
        "row JSON string capacity overflow".into(),
    ))
}

impl Cursor<'_> {
    pub(in crate::runtime::accounted::json_arena) fn read_row_string_observing(
        &mut self,
        mut observe: impl FnMut(),
    ) -> Result<DecodedText, CursorError> {
        self.controls
            .check_interruption()
            .map_err(CursorError::Interrupted)?;
        if self.peek() != Some(b'"') {
            return Err(self.syntax(ErrorKind::ExpectedQuote));
        }
        let _consumed = self.next()?;
        let mut bytes = ByteBuffer::new(&self.controls)?;
        loop {
            let byte = self
                .next()?
                .ok_or_else(|| self.syntax(ErrorKind::EofString))?;
            match byte {
                b'"' => break,
                b'\\' => {
                    let value = self.read_escape()?;
                    let mut encoded = [0_u8; 4];
                    for byte in value.encode_utf8(&mut encoded).bytes() {
                        bytes.push(byte, &self.controls, &mut observe)?;
                    }
                }
                0..=0x1f => return Err(self.syntax(ErrorKind::Control)),
                _ => bytes.push(byte, &self.controls, &mut observe)?,
            }
        }

        let mut output = DecodedText::new(&self.controls)?;
        let mut start = 0;
        while start < bytes.bytes.len() {
            self.controls
                .check_interruption()
                .map_err(CursorError::Interrupted)?;
            let end = start.saturating_add(1024).min(bytes.bytes.len());
            let valid = match std::str::from_utf8(&bytes.bytes[start..end]) {
                Ok(text) => text,
                Err(error)
                    if error.error_len().is_none()
                        && end < bytes.bytes.len()
                        && error.valid_up_to() > 0 =>
                {
                    std::str::from_utf8(&bytes.bytes[start..start + error.valid_up_to()])
                        .expect("validated UTF-8 prefix")
                }
                Err(error) => {
                    let invalid = start + error.valid_up_to();
                    let mut position = self.position();
                    position.column = position.column.saturating_sub(bytes.bytes.len() - invalid);
                    return Err(CursorError::Syntax(Diagnostic {
                        kind: ErrorKind::InvalidUnicode,
                        position,
                    }));
                }
            };
            for character in valid.chars() {
                output.push(character, &self.controls, &mut observe)?;
            }
            start += valid.len();
        }
        Ok(output)
    }
}
