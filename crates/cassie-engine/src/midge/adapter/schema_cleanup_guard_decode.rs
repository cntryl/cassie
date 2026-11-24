//! Query-owned decoding for deferred index guard records.
use std::io::{self, Read};

use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

pub(in crate::midge::adapter) struct Decoded<T> {
    pub(in crate::midge::adapter) value: T,
    _memory: Option<QueryMemoryReservation>,
}

struct CheckingReader<'a> {
    bytes: &'a [u8],
    position: usize,
    controls: Option<&'a QueryExecutionControls>,
}

impl Read for CheckingReader<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if self.position.is_multiple_of(4096) {
            if let Some(controls) = self.controls {
                controls
                    .check_interruption()
                    .map_err(|_| io::Error::other("query interrupted"))?;
            }
        }
        let remaining = &self.bytes[self.position..];
        let count = remaining
            .len()
            .min(output.len())
            .min(4096 - self.position % 4096);
        output[..count].copy_from_slice(&remaining[..count]);
        self.position += count;
        Ok(count)
    }
}

pub(in crate::midge::adapter) fn decode<T: serde::de::DeserializeOwned>(
    raw: &[u8],
    controls: Option<&QueryExecutionControls>,
    label: &str,
) -> Result<Decoded<T>, CassieError> {
    let memory = controls
        .map(|controls| {
            controls.check_interruption()?;
            controls.reserve_query_memory(crate::runtime::accounted::json::serialized_decode_bytes(
                raw.len(),
            )?)
        })
        .transpose()?;
    let value = match serde_json::from_reader(CheckingReader {
        bytes: raw,
        position: 0,
        controls,
    }) {
        Ok(value) => value,
        Err(error) => {
            if let Some(controls) = controls {
                controls.check_interruption()?;
            }
            return Err(CassieError::Parse(format!("invalid {label}: {error}")));
        }
    };
    Ok(Decoded {
        value,
        _memory: memory,
    })
}

pub(in crate::midge::adapter) fn names_match(
    stored: &str,
    requested: &str,
    controls: Option<&QueryExecutionControls>,
) -> Result<bool, CassieError> {
    if let Some(controls) = controls {
        crate::catalog::name_matches_controlled(stored, requested, controls)
    } else {
        Ok(crate::catalog::name_matches(stored, requested))
    }
}
