use std::time::Duration;

use crate::types::DataType;

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct ColumnMeta {
    pub name: String,
    pub data_type: String,
    pub type_oid: i64,
    pub typlen: i16,
    pub atttypmod: i32,
    pub format_code: i16,
    pub nullable: bool,
    /// Whether the column carries a record (named composite or anonymous
    /// `ROW(...)`), which is the only family with a user-range binary codec.
    pub is_record: bool,
    /// The descriptor behind a named record column, which pgwire needs to
    /// send the rendered record text in binary `record_send` layout.
    #[serde(skip)]
    pub record_type: Option<crate::types::CompositeRef>,
    /// The enum type's OID for a named enum column (`t`) or an enum array
    /// column (`t[]`; then `type_oid` is the array OID). User-range type OIDs
    /// have no builtin codec, so pgwire sends the label (`enum_send`) and
    /// `array_send` over labels; see [`Self::enum_column`]. A bare OID keeps
    /// the field inside the struct's existing padding.
    #[serde(skip)]
    pub enum_element_oid: Option<std::num::NonZeroU32>,
}

/// The wire identity of an enum or enum-array result column (#827).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnumColumn {
    /// `pg_type.oid` of the enum type itself (the array element type).
    pub element_oid: u32,
    pub is_array: bool,
}

impl EnumColumn {
    /// The enum description of `data_type`, if it is an enum or enum array.
    #[must_use]
    pub fn of(data_type: &DataType) -> Option<Self> {
        match data_type {
            DataType::Enum(descriptor) => Some(Self {
                element_oid: descriptor.oid,
                is_array: false,
            }),
            DataType::Array(inner) => match inner.as_ref() {
                DataType::Enum(descriptor) => Some(Self {
                    element_oid: descriptor.oid,
                    is_array: true,
                }),
                _ => None,
            },
            _ => None,
        }
    }

    /// The `pg_type.oid` the column reports in `RowDescription`.
    #[must_use]
    pub fn wire_oid(data_type: &DataType) -> Option<i64> {
        match data_type {
            DataType::Enum(descriptor) => Some(i64::from(descriptor.oid)),
            DataType::Array(inner) => match inner.as_ref() {
                DataType::Enum(descriptor) => Some(i64::from(descriptor.array_oid)),
                _ => None,
            },
            _ => None,
        }
    }
}

impl ColumnMeta {
    pub fn text(name: impl Into<String>) -> Self {
        Self::from_data_type(name, &DataType::Text)
    }

    pub fn from_data_type(name: impl Into<String>, data_type: &DataType) -> Self {
        let data_type_name = data_type.type_name();
        let enum_column = EnumColumn::of(data_type);
        let enum_element_oid =
            enum_column.and_then(|column| std::num::NonZeroU32::new(column.element_oid));
        Self {
            name: name.into(),
            data_type: data_type_name,
            // An enum column reports the enum's own OID (its array OID for an
            // array), never `text`.
            type_oid: EnumColumn::wire_oid(data_type).unwrap_or_else(|| data_type.type_oid()),
            typlen: match data_type {
                DataType::Enum(_) => 4,
                _ if enum_column.is_some() => -1,
                _ => data_type.typlen(),
            },
            atttypmod: data_type.atttypmod(),
            format_code: 0,
            nullable: true,
            is_record: matches!(data_type, DataType::Composite(_)),
            record_type: match data_type {
                DataType::Composite(descriptor) => Some(descriptor.clone()),
                _ => None,
            },
            enum_element_oid,
        }
    }

    /// The enum wire description of this column, if it is an enum or enum
    /// array column: an array column reports the array OID, not the enum's.
    #[must_use]
    pub fn enum_column(&self) -> Option<EnumColumn> {
        let element_oid = self.enum_element_oid?.get();
        Some(EnumColumn {
            element_oid,
            is_array: self.type_oid != i64::from(element_oid),
        })
    }

    /// Whether the column's user-range type OID has a pgwire codec of its own
    /// (a record, enum or enum array) rather than a builtin one.
    #[must_use]
    pub fn user_codec(&self) -> bool {
        self.is_record || self.enum_element_oid.is_some()
    }

    pub(crate) fn is_vector(&self) -> bool {
        self.data_type.eq_ignore_ascii_case("vector")
            || self
                .data_type
                .get(..7)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("vector("))
    }
}

pub use super::query_result::QueryResult;

#[cfg(test)]
#[path = "enum_column_tests.rs"]
mod enum_column_tests;

#[cfg(test)]
#[path = "result_ownership_tests.rs"]
mod result_ownership_tests;

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
    if duration.is_zero() {
        return 0;
    }
    duration.as_micros().try_into().unwrap_or(u64::MAX).max(1)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::duration_micros;

    #[test]
    fn should_report_sub_microsecond_execution_stages_as_one_microsecond() {
        // Arrange
        let measured = Duration::from_nanos(1);

        // Act
        let micros = duration_micros(measured);

        // Assert
        assert_eq!(micros, 1);
        assert_eq!(duration_micros(Duration::ZERO), 0);
    }
}

#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    #[error("value out of range: overflow")]
    FloatPowerOverflow,

    #[error("value out of range: underflow")]
    FloatPowerUnderflow,

    #[error("{0}")]
    General(String),

    #[error(transparent)]
    Cassie(#[from] crate::app::CassieError),
}
