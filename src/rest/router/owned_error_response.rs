//! Controlled JSON response for an admitted NUMERIC input error.
use std::io::{self, Write};
use std::mem::size_of;

use bytes::Bytes;
use hyper::{header::CONTENT_TYPE, Response, StatusCode};

use crate::app::{CassieError, OwnedNumericLiteralError};
use crate::executor::retained_memory::add;
use crate::rest::body::{full_body, RestBody};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

#[cfg(test)]
#[path = "owned_error_response_tests.rs"]
mod tests;

pub(super) fn numeric_error_response(
    error: &OwnedNumericLiteralError,
) -> Result<Response<RestBody>, CassieError> {
    let controls = error.controls();
    let mut counter = Writer {
        controls,
        bytes: None,
        length: 0,
        failure: None,
    };
    encode_error(error.message(), &mut counter)?;
    // Bytes::from_owner and the boxed HTTP body have their own allocations.
    let overhead = add(size_of::<EncodedError>(), 2 * size_of::<usize>())?;
    let body = add(size_of::<http_body_util::Full<Bytes>>(), size_of::<usize>())?;
    let memory = controls.reserve_query_memory(add(counter.length, add(overhead, body)?)?)?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(counter.length).map_err(|error| {
        CassieError::ResourceLimit(format!("unable to allocate REST error response: {error}"))
    })?;
    if bytes.capacity() > counter.length {
        return Err(CassieError::ResourceLimit(
            "allocator exceeded admitted REST error capacity".into(),
        ));
    }
    let mut writer = Writer {
        controls,
        bytes: Some(&mut bytes),
        length: 0,
        failure: None,
    };
    encode_error(error.message(), &mut writer)?;
    if writer.length != counter.length {
        return Err(CassieError::Execution(
            "REST error serialization changed between passes".into(),
        ));
    }
    controls.check_interruption()?;
    let bytes = Bytes::from_owner(EncodedError {
        bytes,
        _memory: memory,
    });
    let mut response = Response::new(full_body(bytes));
    *response.status_mut() = StatusCode::BAD_REQUEST;
    response.headers_mut().insert(
        CONTENT_TYPE,
        hyper::header::HeaderValue::from_static("application/json"),
    );
    Ok(response)
}

struct EncodedError {
    bytes: Vec<u8>,
    _memory: QueryMemoryReservation,
}
impl AsRef<[u8]> for EncodedError {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

struct Writer<'a> {
    controls: &'a QueryExecutionControls,
    bytes: Option<&'a mut Vec<u8>>,
    length: usize,
    failure: Option<CassieError>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum EncodingPhase {
    Scan,
    Emit,
}

fn encode_error(message: &str, writer: &mut Writer<'_>) -> Result<(), CassieError> {
    encode_error_observing(message, writer, |_| {})
}

fn encode_error_observing(
    message: &str,
    writer: &mut Writer<'_>,
    mut progress: impl FnMut(EncodingPhase),
) -> Result<(), CassieError> {
    writer
        .write_all(b"{\"error\":\"")
        .map_err(|_| writer.take_failure())?;
    let mut start = 0;
    for (index, byte) in message.bytes().enumerate() {
        if index.is_multiple_of(1024) {
            if index != 0 {
                progress(EncodingPhase::Scan);
            }
            writer.controls.check_interruption()?;
        }
        let escape: Option<&[u8]> = match byte {
            b'\"' => Some(b"\\\""),
            b'\\' => Some(b"\\\\"),
            b'\x08' => Some(b"\\b"),
            b'\x0c' => Some(b"\\f"),
            b'\n' => Some(b"\\n"),
            b'\r' => Some(b"\\r"),
            b'\t' => Some(b"\\t"),
            _ => None,
        };
        if byte < 0x20 || escape.is_some() {
            write_observing(writer, &message.as_bytes()[start..index], &mut progress)?;
            if let Some(escape) = escape {
                writer
                    .write_all(escape)
                    .map_err(|_| writer.take_failure())?;
            } else {
                const HEX: &[u8; 16] = b"0123456789abcdef";
                let encoded = [
                    b'\\',
                    b'u',
                    b'0',
                    b'0',
                    HEX[usize::from(byte >> 4)],
                    HEX[usize::from(byte & 15)],
                ];
                writer
                    .write_all(&encoded)
                    .map_err(|_| writer.take_failure())?;
            }
            start = index + 1;
        }
    }
    write_observing(writer, &message.as_bytes()[start..], &mut progress)?;
    writer
        .write_all(b"\"}")
        .map_err(|_| writer.take_failure())?;
    Ok(())
}

fn write_observing(
    writer: &mut Writer<'_>,
    bytes: &[u8],
    progress: &mut impl FnMut(EncodingPhase),
) -> Result<(), CassieError> {
    for chunk in bytes.chunks(1024) {
        writer.write_all(chunk).map_err(|_| writer.take_failure())?;
        progress(EncodingPhase::Emit);
        writer.controls.check_interruption()?;
    }
    Ok(())
}

impl Writer<'_> {
    fn take_failure(&mut self) -> CassieError {
        self.failure
            .take()
            .unwrap_or_else(|| CassieError::Execution("REST error serialization failed".into()))
    }
}
impl Write for Writer<'_> {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        for chunk in input.chunks(4096) {
            if let Err(error) = self.controls.check_interruption() {
                self.failure = Some(error);
                return Err(io::Error::other("REST error encoding interrupted"));
            }
            let length = match add(self.length, chunk.len()) {
                Ok(length) => length,
                Err(error) => {
                    self.failure = Some(error);
                    return Err(io::Error::other("REST error encoding overflow"));
                }
            };
            if let Some(bytes) = &mut self.bytes {
                if length > bytes.capacity() {
                    self.failure = Some(CassieError::Execution(
                        "REST error encoding exceeded its admission".into(),
                    ));
                    return Err(io::Error::other("REST error encoding length changed"));
                }
                bytes.extend_from_slice(chunk);
            }
            self.length = length;
        }
        Ok(input.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
