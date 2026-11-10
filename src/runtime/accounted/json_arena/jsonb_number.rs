//! Controlled decimal output for already validated JSON number tokens.
use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::extended::numeric::{MAX_DSCALE, MAX_INTEGER_DIGITS};

#[cfg(test)]
#[path = "jsonb_number_tests.rs"]
mod tests;

#[derive(Debug)]
pub(in crate::runtime::accounted::json_arena) enum FormatError {
    Overflow,
    Control(CassieError),
}

impl From<CassieError> for FormatError {
    fn from(error: CassieError) -> Self {
        Self::Control(error)
    }
}

#[derive(Clone, Copy)]
pub(in crate::runtime::accounted::json_arena) enum Progress {
    Scan,
    Output,
}

pub(in crate::runtime::accounted::json_arena) struct OwnedNumberText {
    text: String,
    memory: QueryMemoryReservation,
}

impl OwnedNumberText {
    pub(in crate::runtime::accounted::json_arena) fn as_str(&self) -> &str {
        &self.text
    }

    pub(in crate::runtime::accounted::json_arena) fn into_parts(
        self,
    ) -> (String, QueryMemoryReservation) {
        (self.text, self.memory)
    }
}

#[cfg(test)]
pub(super) fn format_number(
    input: &str,
    controls: &QueryExecutionControls,
) -> Result<OwnedNumberText, FormatError> {
    format_number_with_observer(input, controls, |_| {})
}

#[cfg(test)]
fn format_number_observed(
    input: &str,
    controls: &QueryExecutionControls,
    observer: impl FnMut(Progress),
) -> Result<OwnedNumberText, FormatError> {
    format_number_with_observer(input, controls, observer)
}

// This helper is reachable only from the arena's validated number nodes. It is
// not a replacement for NUMERIC's public syntax validation.
pub(in crate::runtime::accounted::json_arena) fn format_number_with_observer(
    input: &str,
    controls: &QueryExecutionControls,
    mut observer: impl FnMut(Progress),
) -> Result<OwnedNumberText, FormatError> {
    controls.check_interruption()?;
    let layout = layout(input, controls, &mut observer)?;
    let memory = controls.reserve_query_memory(layout.length)?;
    let mut text = String::new();
    text.try_reserve_exact(layout.length)
        .map_err(|_| CassieError::ResourceLimit("JSONB numeric output allocation failed".into()))?;
    if text.capacity() > layout.length {
        return Err(CassieError::ResourceLimit(
            "JSONB numeric output exceeded its admission".into(),
        )
        .into());
    }
    let mut writer = Writer {
        text,
        memory,
        controls,
        observer: &mut observer,
        emitted: 0,
    };
    write_value(input.as_bytes(), &layout, &mut writer)?;
    controls.check_interruption()?;
    Ok(OwnedNumberText {
        text: writer.text,
        memory: writer.memory,
    })
}

struct Layout {
    first: Option<usize>,
    significant: usize,
    integer_digits: usize,
    displayed_scale: usize,
    scale: i64,
    negative: bool,
    length: usize,
}

fn layout(
    input: &str,
    controls: &QueryExecutionControls,
    observer: &mut impl FnMut(Progress),
) -> Result<Layout, FormatError> {
    let bytes = input.as_bytes();
    let mut fraction = 0_usize;
    let mut in_fraction = false;
    let mut in_exponent = false;
    let mut exponent_negative = false;
    let mut magnitude = 0_i64;
    let mut first = None;
    let mut significant = 0_usize;
    for (index, byte) in bytes.iter().copied().enumerate() {
        if index > 0 && index.is_multiple_of(1024) {
            observer(Progress::Scan);
            controls.check_interruption()?;
        }
        match byte {
            b'e' | b'E' => in_exponent = true,
            b'.' => in_fraction = true,
            b'-' if in_exponent => exponent_negative = true,
            b'-' | b'+' => {}
            b'0'..=b'9' if in_exponent => {
                magnitude = magnitude
                    .saturating_mul(10)
                    .saturating_add(i64::from(byte - b'0'));
            }
            b'0'..=b'9' => {
                if in_fraction {
                    fraction = fraction.checked_add(1).ok_or(FormatError::Overflow)?;
                }
                if first.is_some() {
                    significant = significant.checked_add(1).ok_or(FormatError::Overflow)?;
                } else if byte != b'0' {
                    first = Some(index);
                    significant = 1;
                }
            }
            _ => unreachable!("number nodes contain validated ASCII tokens"),
        }
    }
    controls.check_interruption()?;
    if magnitude > i64::from(i32::MAX / 2) {
        return Err(FormatError::Overflow);
    }
    let exponent = if exponent_negative {
        -magnitude
    } else {
        magnitude
    };
    let scale = i64::try_from(fraction)
        .map_err(|_| FormatError::Overflow)?
        .checked_sub(exponent)
        .ok_or(FormatError::Overflow)?;
    if scale > i64::from(MAX_DSCALE) {
        return Err(FormatError::Overflow);
    }
    let integer_digits = if significant == 0 {
        0
    } else {
        i64::try_from(significant)
            .map_err(|_| FormatError::Overflow)?
            .checked_sub(scale)
            .ok_or(FormatError::Overflow)?
            .max(0)
    };
    let integer_digits = usize::try_from(integer_digits).map_err(|_| FormatError::Overflow)?;
    if integer_digits > MAX_INTEGER_DIGITS {
        return Err(FormatError::Overflow);
    }
    let displayed_scale = usize::try_from(scale.max(0)).map_err(|_| FormatError::Overflow)?;
    let negative = first.is_some() && bytes.first() == Some(&b'-');
    let length = integer_digits
        .max(1)
        .checked_add(displayed_scale)
        .and_then(|n| n.checked_add(usize::from(displayed_scale != 0)))
        .and_then(|n| n.checked_add(usize::from(negative)))
        .ok_or(FormatError::Overflow)?;
    Ok(Layout {
        first,
        significant,
        integer_digits,
        displayed_scale,
        scale,
        negative,
        length,
    })
}

fn write_value<F: FnMut(Progress)>(
    bytes: &[u8],
    layout: &Layout,
    writer: &mut Writer<'_, F>,
) -> Result<(), FormatError> {
    if layout.negative {
        writer.push('-')?;
    }
    match layout.first {
        None => {
            writer.push('0')?;
            if layout.displayed_scale != 0 {
                writer.push('.')?;
                writer.zeros(layout.displayed_scale)?;
            }
        }
        Some(first) => {
            if layout.integer_digits == 0 {
                writer.push('0')?;
                writer.push('.')?;
                writer.zeros(layout.displayed_scale - layout.significant)?;
            }
            let mut digits = 0_usize;
            for byte in bytes[first..].iter().copied() {
                if matches!(byte, b'e' | b'E') {
                    break;
                }
                if byte == b'.' {
                    continue;
                }
                if layout.integer_digits != 0
                    && layout.displayed_scale != 0
                    && digits == layout.integer_digits
                {
                    writer.push('.')?;
                }
                writer.push(char::from(byte))?;
                digits += 1;
            }
            if layout.scale < 0 {
                writer.zeros(usize::try_from(-layout.scale).map_err(|_| FormatError::Overflow)?)?;
            }
        }
    }
    Ok(())
}

struct Writer<'a, F> {
    text: String,
    memory: QueryMemoryReservation,
    controls: &'a QueryExecutionControls,
    observer: &'a mut F,
    emitted: usize,
}

impl<F: FnMut(Progress)> Writer<'_, F> {
    fn push(&mut self, value: char) -> Result<(), FormatError> {
        self.text.push(value);
        self.emitted += 1;
        if self.emitted.is_multiple_of(1024) {
            (self.observer)(Progress::Output);
            self.controls.check_interruption()?;
        }
        Ok(())
    }

    fn zeros(&mut self, count: usize) -> Result<(), FormatError> {
        for _ in 0..count {
            self.push('0')?;
        }
        Ok(())
    }
}
