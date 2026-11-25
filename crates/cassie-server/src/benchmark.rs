//! Pgwire-specific production kernels used by Cassie's benchmark executables.

use crate::types::{DataType, Value};

pub use cassie_engine::benchmark::{
    ExecutorKernel, KernelObservation, RowCodecKernel, RowKeyKernel, SubsystemExecutorKernel,
};

/// Production binary pgwire data-row encoder input.
pub struct PgwireRowCodecKernel {
    row: Vec<Value>,
    columns: Vec<crate::executor::ColumnMeta>,
}

impl PgwireRowCodecKernel {
    #[must_use]
    pub fn sample(index: usize) -> Self {
        Self {
            row: vec![
                Value::String(format!("doc-{index}")),
                Value::Int64(i64::try_from(index).expect("pgwire fixture index should fit i64")),
            ],
            columns: vec![
                crate::executor::ColumnMeta::text("id"),
                crate::executor::ColumnMeta::from_data_type("score", &DataType::Int),
            ],
        }
    }

    /// Encodes one row through the frame writer used by live pgwire connections.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let row = self.row.clone();
        crate::pgwire::connection::benchmark_encode_data_row(&row, &self.columns, &[])
            .expect("benchmark pgwire row should encode")
    }
}

/// Production binary pgwire frontend decoder inputs.
pub struct PgwireFrontendCodecKernel {
    frames: Vec<(u8, Vec<u8>)>,
}

impl PgwireFrontendCodecKernel {
    #[must_use]
    pub fn with_frames(frames: usize) -> Self {
        let frames = (0..frames)
            .map(|index| frontend_fixture(index % 5))
            .collect();
        Self { frames }
    }

    /// Decodes every fixture with the parser used by live pgwire connections.
    #[must_use]
    pub fn decode(&self) -> usize {
        self.frames
            .iter()
            .map(|(tag, payload)| {
                crate::pgwire::connection::benchmark_decode_frontend(*tag, payload.clone())
                    .expect("benchmark pgwire frontend frame should decode")
            })
            .sum()
    }

    #[must_use]
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }
}

/// Production pgwire bind-parameter decoder inputs.
pub struct PgwireParameterBindingKernel {
    parameters: Vec<(Vec<u8>, i16, i32)>,
}

impl PgwireParameterBindingKernel {
    #[must_use]
    pub fn with_parameters(parameters: usize) -> Self {
        let fixtures: [(&[u8], i16, i32); 4] = [
            (b"42", 0, 23),
            (b"alpha", 0, 25),
            (b"true", 0, 16),
            (b"3.5", 0, 701),
        ];
        let parameters = (0..parameters)
            .map(|index| {
                let (value, format, oid) = fixtures[index % fixtures.len()];
                (value.to_vec(), format, oid)
            })
            .collect();
        Self { parameters }
    }

    /// Decodes every value through the production extended-query bind path.
    #[must_use]
    pub fn decode(&self) -> Vec<Value> {
        self.parameters
            .iter()
            .map(|(parameter, format, oid)| {
                crate::pgwire::connection::benchmark_decode_parameter(parameter, *format, *oid)
                    .expect("benchmark bind parameter should decode")
            })
            .collect()
    }

    #[must_use]
    pub fn parameter_count(&self) -> usize {
        self.parameters.len()
    }
}

fn frontend_fixture(index: usize) -> (u8, Vec<u8>) {
    match index {
        0 => {
            let mut payload = cstrings(&["bench_stmt", "SELECT $1::INT AS value"]);
            payload.extend_from_slice(&1_i16.to_be_bytes());
            payload.extend_from_slice(&23_i32.to_be_bytes());
            (b'P', payload)
        }
        1 => {
            let mut payload = cstrings(&["bench_portal", "bench_stmt"]);
            payload.extend_from_slice(&0_i16.to_be_bytes());
            payload.extend_from_slice(&1_i16.to_be_bytes());
            payload.extend_from_slice(&1_i32.to_be_bytes());
            payload.push(b'7');
            payload.extend_from_slice(&0_i16.to_be_bytes());
            (b'B', payload)
        }
        2 => {
            let mut payload = vec![b'S'];
            payload.extend_from_slice(b"bench_stmt\0");
            (b'D', payload)
        }
        3 => {
            let mut payload = cstrings(&["bench_portal"]);
            payload.extend_from_slice(&20_i32.to_be_bytes());
            (b'E', payload)
        }
        _ => (b'S', Vec::new()),
    }
}

fn cstrings(values: &[&str]) -> Vec<u8> {
    let mut payload = Vec::new();
    for value in values {
        payload.extend_from_slice(value.as_bytes());
        payload.push(0);
    }
    payload
}
