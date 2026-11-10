//! Checked recognition of the single explicit dimension supported by owned paths.
use super::Progress;
use crate::executor::QueryError;
use crate::runtime::QueryExecutionControls;

struct Cursor<'a, 'b, F> {
    input: &'a str,
    position: usize,
    controls: &'a QueryExecutionControls,
    observe: &'b mut F,
}
impl<F: FnMut(Progress)> Cursor<'_, '_, F> {
    fn peek(&self) -> Option<char> {
        self.input[self.position..].chars().next()
    }
    fn next(&mut self) -> Result<Option<char>, QueryError> {
        self.controls.check_interruption()?;
        let value = self.peek();
        if let Some(value) = value {
            self.position += value.len_utf8();
            (self.observe)(Progress::Dimension);
            self.controls.check_interruption()?;
        }
        Ok(value)
    }
    fn whitespace(&mut self, ascii: bool) -> Result<(), QueryError> {
        while self.peek().is_some_and(|value| {
            if ascii {
                value.is_ascii_whitespace()
            } else {
                value.is_whitespace()
            }
        }) {
            let _consumed = self.next()?;
        }
        Ok(())
    }
    fn bound(&mut self, delimiter: char) -> Result<Option<i32>, QueryError> {
        self.whitespace(false)?;
        let negative = match self.peek() {
            Some('-') => {
                let _consumed = self.next()?;
                true
            }
            Some('+') => {
                let _consumed = self.next()?;
                false
            }
            _ => false,
        };
        let mut magnitude = 0_i64;
        let mut digits = false;
        while let Some(value) = self.peek().filter(char::is_ascii_digit) {
            let _consumed = self.next()?;
            digits = true;
            magnitude = magnitude
                .saturating_mul(10)
                .saturating_add(i64::from(u32::from(value) - u32::from('0')));
        }
        self.whitespace(false)?;
        if !digits || self.peek() != Some(delimiter) {
            return Ok(None);
        }
        let _consumed = self.next()?;
        let signed = if negative { -magnitude } else { magnitude };
        Ok(i32::try_from(signed).ok())
    }
}

pub(super) fn body<'a, F: FnMut(Progress)>(
    input: &'a str,
    controls: &'a QueryExecutionControls,
    observe: &mut F,
) -> Result<Option<(usize, &'a str)>, QueryError> {
    let mut cursor = Cursor {
        input,
        position: 0,
        controls,
        observe,
    };
    cursor.whitespace(false)?;
    if cursor.peek() != Some('[') {
        return Ok(None);
    }
    let _consumed = cursor.next()?;
    let Some(lower) = cursor.bound(':')? else {
        return Ok(None);
    };
    let Some(upper) = cursor.bound(']')? else {
        return Ok(None);
    };
    let length = i64::from(upper) - i64::from(lower) + 1;
    let Ok(length) = i32::try_from(length) else {
        return Ok(None);
    };
    let Ok(length) = usize::try_from(length) else {
        return Ok(None);
    };
    cursor.whitespace(true)?;
    if cursor.peek() != Some('=') {
        return Ok(None);
    }
    let _consumed = cursor.next()?;
    Ok(Some((length, &input[cursor.position..])))
}
