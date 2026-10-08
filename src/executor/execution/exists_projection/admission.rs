//! Reserve temporary EXISTS state before cloning it.

use std::io::{self, Write};

use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

struct CountingWriter<'a> {
    bytes: usize,
    controls: &'a QueryExecutionControls,
    aborted: Option<CassieError>,
    depth: usize,
    quoted: bool,
    escaped: bool,
}

impl Write for CountingWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.aborted = if self.controls.is_cancelled() {
            Some(CassieError::QueryCancelled)
        } else if self.controls.is_timed_out() {
            Some(CassieError::DeadlineExceeded)
        } else {
            None
        };
        if self.aborted.is_some() {
            return Err(io::Error::other("EXISTS state counting interrupted"));
        }
        for byte in bytes {
            if self.quoted {
                if self.escaped {
                    self.escaped = false;
                } else if *byte == b'\\' {
                    self.escaped = true;
                } else if *byte == b'"' {
                    self.quoted = false;
                }
            } else {
                match byte {
                    b'"' => self.quoted = true,
                    b'{' | b'[' => {
                        self.depth += 1;
                        if self.depth > 512 {
                            self.aborted = Some(CassieError::ResourceLimit(
                                "EXISTS state nesting exceeds 512 serialized containers".into(),
                            ));
                            return Err(io::Error::other("EXISTS state nesting exceeded"));
                        }
                    }
                    b'}' | b']' => self.depth = self.depth.saturating_sub(1),
                    _ => {}
                }
            }
        }
        self.bytes = self
            .bytes
            .checked_add(bytes.len())
            .ok_or_else(|| io::Error::other("EXISTS state size overflow"))?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) fn reserve_clone(
    value: &impl serde::Serialize,
    controls: &QueryExecutionControls,
) -> Result<QueryMemoryReservation, CassieError> {
    if controls.is_cancelled() {
        return Err(CassieError::QueryCancelled);
    }
    if controls.is_timed_out() {
        return Err(CassieError::DeadlineExceeded);
    }
    let mut writer = CountingWriter {
        bytes: 0,
        controls,
        aborted: None,
        depth: 0,
        quoted: false,
        escaped: false,
    };
    // Count without retaining a serialized buffer. Each serialized expression
    // node occupies multiple bytes; 256 bytes per byte conservatively covers
    // enum payloads, Vec/String allocation, and the physical-plan reconstruction.
    if let Err(error) = serde_json::to_writer(&mut writer, value) {
        return Err(writer
            .aborted
            .unwrap_or_else(|| CassieError::ResourceLimit(error.to_string())));
    }
    let bytes = writer
        .bytes
        .checked_mul(256)
        .ok_or_else(|| CassieError::ResourceLimit("EXISTS state size overflow".into()))?;
    controls.reserve_query_memory(bytes)
}
