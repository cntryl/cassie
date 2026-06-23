use std::time::Duration;

use crate::types::{DataType, Value};

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct ColumnMeta {
    pub name: String,
    pub data_type: String,
    pub type_oid: i64,
    pub typlen: i16,
    pub atttypmod: i32,
    pub format_code: i16,
    pub nullable: bool,
}

impl ColumnMeta {
    pub fn text(name: impl Into<String>) -> Self {
        Self::from_data_type(name, DataType::Text)
    }

    pub fn from_data_type(name: impl Into<String>, data_type: DataType) -> Self {
        let data_type_name = data_type.type_name();
        Self {
            name: name.into(),
            data_type: data_type_name,
            type_oid: data_type.type_oid(),
            typlen: data_type.typlen(),
            atttypmod: data_type.atttypmod(),
            format_code: 0,
            nullable: true,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct QueryResult {
    pub columns: Vec<ColumnMeta>,
    pub rows: Vec<Vec<Value>>,
    pub command: String,
}

#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct ExecutionBreakdownMicros {
    pub scan_us: u64,
    pub row_decode_us: u64,
    pub filter_us: u64,
    pub projection_us: u64,
    pub sort_us: u64,
    pub result_build_us: u64,
    pub stats_us: u64,
}

#[derive(Debug, Clone)]
pub struct ExecutionBreakdownOutput {
    pub result: QueryResult,
    pub breakdown: ExecutionBreakdownMicros,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ExecutionBreakdownDurations {
    pub(crate) scan: Duration,
    pub(crate) row_decode: Duration,
    pub(crate) filter: Duration,
    pub(crate) projection: Duration,
    pub(crate) sort: Duration,
    pub(crate) result_build: Duration,
    pub(crate) stats: Duration,
}

impl ExecutionBreakdownDurations {
    pub(crate) fn into_micros(self) -> ExecutionBreakdownMicros {
        ExecutionBreakdownMicros {
            scan_us: duration_micros(self.scan),
            row_decode_us: duration_micros(self.row_decode),
            filter_us: duration_micros(self.filter),
            projection_us: duration_micros(self.projection),
            sort_us: duration_micros(self.sort),
            result_build_us: duration_micros(self.result_build),
            stats_us: duration_micros(self.stats),
        }
    }
}

fn duration_micros(duration: Duration) -> u64 {
    duration.as_micros().try_into().unwrap_or(u64::MAX)
}

#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    #[error("{0}")]
    General(String),

    #[error(transparent)]
    Cassie(#[from] crate::app::CassieError),
}
