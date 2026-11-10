//! Fixed-stack inet and cidr output for admitted binary COPY fields.

use std::fmt::{self, Write as _};

use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::{extended::network::NetAddr, typed::TypedValue, Value};

struct Output {
    bytes: [u8; 48],
    len: usize,
}

impl fmt::Write for Output {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let end = self.len.checked_add(text.len()).ok_or(fmt::Error)?;
        let target = self.bytes.get_mut(self.len..end).ok_or(fmt::Error)?;
        target.copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}

fn format_v6(out: &mut Output, addr: &[u8; 16]) -> fmt::Result {
    let mut words = [0_u16; 8];
    for (word, pair) in words.iter_mut().zip(addr.as_chunks::<2>().0) {
        *word = u16::from_be_bytes(*pair);
    }
    let (mut best, mut current) = ((None::<usize>, 0_usize), (None::<usize>, 0_usize));
    for (index, word) in words.iter().enumerate() {
        if *word == 0 {
            current = (current.0.or(Some(index)), current.1 + 1);
        } else if current.0.is_some() {
            if best.0.is_none() || current.1 > best.1 {
                best = current;
            }
            current = (None, 0);
        }
    }
    if current.0.is_some() && (best.0.is_none() || current.1 > best.1) {
        best = current;
    }
    if best.1 < 2 {
        best = (None, 0);
    }
    for (index, word) in words.iter().enumerate() {
        if let Some(base) = best.0 {
            if (base..base + best.1).contains(&index) {
                if index == base {
                    out.write_char(':')?;
                }
                continue;
            }
        }
        if index != 0 {
            out.write_char(':')?;
        }
        if index == 6
            && best.0 == Some(0)
            && (best.1 == 6
                || (best.1 == 7 && words[7] != 1)
                || (best.1 == 5 && words[5] == 0xffff))
        {
            write!(out, "{}.{}.{}.{}", addr[12], addr[13], addr[14], addr[15])?;
            break;
        }
        write!(out, "{word:x}")?;
    }
    if best.0.is_some_and(|base| base + best.1 == 8) {
        out.write_char(':')?;
    }
    Ok(())
}

fn format_network(value: NetAddr, is_cidr: bool, out: &mut Output) -> fmt::Result {
    if value.is_v4() {
        let bytes = value.bytes();
        write!(out, "{}.{}.{}.{}", bytes[0], bytes[1], bytes[2], bytes[3])?;
    } else {
        format_v6(out, &value.addr)?;
    }
    if is_cidr || value.bits != value.width() {
        write!(out, "/{}", value.bits)?;
    }
    Ok(())
}

pub(super) fn decode(
    oid: i64,
    bytes: &[u8],
    column: usize,
    controls: &QueryExecutionControls,
) -> Result<Option<(String, QueryMemoryReservation)>, CassieError> {
    if !matches!(oid, 869 | 650) {
        return Ok(None);
    }
    controls.check_interruption()?;
    let value = super::super::super::codecs::binary_to_value(bytes, oid).map_err(|error| {
        CassieError::Parse(format!(
            "COPY binary field {} is invalid: {error}",
            column + 1
        ))
    })?;
    controls.check_interruption()?;
    let (value, is_cidr) = match value {
        Value::Typed(TypedValue::Inet(value)) => (value, false),
        Value::Typed(TypedValue::Cidr(value)) => (value, true),
        _ => {
            return Err(CassieError::Execution(
                "unexpected binary COPY inet value".into(),
            ));
        }
    };
    let mut output = Output {
        bytes: [0; 48],
        len: 0,
    };
    format_network(value, is_cidr, &mut output)
        .map_err(|_| CassieError::Execution("binary COPY inet output exceeded stack".into()))?;
    let memory = controls.reserve_query_memory(output.len)?;
    let mut text = String::new();
    text.try_reserve_exact(output.len)
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if text.capacity() > output.len {
        return Err(CassieError::ResourceLimit(
            "allocator exceeded admitted COPY inet capacity".into(),
        ));
    }
    let formatted = std::str::from_utf8(&output.bytes[..output.len])
        .map_err(|_| CassieError::Execution("invalid binary COPY inet output".into()))?;
    text.push_str(formatted);
    controls.check_interruption()?;
    Ok(Some((text, memory)))
}
