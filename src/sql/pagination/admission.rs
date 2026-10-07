//! Reserve temporary direct-executor plan state before cloning it.

use std::io::{self, Write};

use crate::app::CassieError;
use crate::planner::logical::LogicalPlan;
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
            return Err(io::Error::other("pagination plan counting interrupted"));
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
                                "pagination plan nesting exceeds 512 serialized containers".into(),
                            ));
                            return Err(io::Error::other("pagination plan nesting exceeded"));
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
            .ok_or_else(|| io::Error::other("pagination plan size overflow"))?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) fn reserve_plan_clone(
    plan: &LogicalPlan,
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
    if let Err(error) = serde_json::to_writer(&mut writer, plan) {
        return Err(writer
            .aborted
            .unwrap_or_else(|| CassieError::ResourceLimit(error.to_string())));
    }
    let bytes = writer
        .bytes
        .checked_mul(256)
        .ok_or_else(|| CassieError::ResourceLimit("pagination plan size overflow".into()))?;
    controls.reserve_query_memory(bytes)
}

#[cfg(test)]
mod tests {
    use super::{CountingWriter, QueryExecutionControls};
    use std::io::Write;

    #[test]
    fn should_preserve_quoted_counting_state_across_write_chunks() {
        // Arrange
        let limits = crate::config::CassieRuntimeConfig::default().limits;
        let controls = QueryExecutionControls::from_limits(&limits, std::time::Instant::now());
        let mut writer = CountingWriter {
            bytes: 0,
            controls: &controls,
            aborted: None,
            depth: 0,
            quoted: false,
            escaped: false,
        };

        // Act
        writer.write_all(b"{\"text\":\"").expect("open string");
        writer.write_all(&[b'{'; 1_024]).expect("quoted braces");
        writer.write_all(b"\\").expect("escape begins");
        writer
            .write_all(b"\"")
            .expect("escaped quote in next chunk");
        writer.write_all(&[b']'; 1_024]).expect("still quoted");
        writer.write_all(b"\"}").expect("close string and object");

        // Assert
        assert_eq!(writer.depth, 0);
        assert!(!writer.quoted);
        assert!(!writer.escaped);
        assert!(writer.aborted.is_none());
    }
}
