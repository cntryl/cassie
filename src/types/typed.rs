//! Native runtime carriers for [`ExtendedType`] values.
//!
//! Inside the engine a value keeps its native form, so comparison, grouping
//! and indexing follow PostgreSQL. At the result boundary it renders as its
//! PostgreSQL text output, matching the text wire format.
use serde::{Deserialize, Serialize};

use super::extended::bit_string::{self, BitString};
use super::extended::network::{self, NetAddr, NetworkKey};
use super::extended::temporal::{self, Interval, TemporalSession, TimeTz};
use super::extended::{numeric, pg_lsn, real, ExtendedType};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TypedValue {
    Real(f32),
    Numeric(numeric::Numeric),
    Inet(NetAddr),
    Cidr(NetAddr),
    Macaddr([u8; 6]),
    Macaddr8([u8; 8]),
    PgLsn(u64),
    Bit(BitString),
    VarBit(BitString),
    /// Days since 2000-01-01 (`i32::MIN`/`MAX` are `-infinity`/`infinity`).
    Date(i32),
    /// Microseconds since midnight.
    Time(i64),
    TimeTz(TimeTz),
    /// Microseconds since 2000-01-01 00:00 (wall clock).
    Timestamp(i64),
    /// Microseconds since 2000-01-01 00:00 UTC.
    TimestampTz(i64),
    Interval(Interval),
}

/// A totally ordered, hashable key with PostgreSQL ordering semantics.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TypedKey {
    /// `float4` with PostgreSQL's total order (NaN sorts last, -0 = 0).
    Real(u32),
    /// `numeric` with PostgreSQL order (NaN last, `1.0` = `1.00`).
    Numeric(numeric::NumericKey),
    /// `inet` and `cidr` share PostgreSQL's `network_cmp` order.
    Network(NetworkKey),
    Macaddr([u8; 6]),
    Macaddr8([u8; 8]),
    PgLsn(u64),
    /// `bit` and `bit varying` share `bit_cmp`: bytes, then bit length.
    Bits(Vec<u8>, usize),
    Date(i32),
    Time(i64),
    TimeTz(i64, i32),
    Timestamp(i64),
    TimestampTz(i64),
    /// 30-day months and 24-hour days, as `interval_cmp` orders them.
    Interval(i128),
}

impl TypedValue {
    #[must_use]
    pub const fn kind(&self) -> ExtendedType {
        match self {
            Self::Real(_) => ExtendedType::Real,
            Self::Numeric(_) => ExtendedType::Numeric(None),
            Self::Inet(_) => ExtendedType::Inet,
            Self::Cidr(_) => ExtendedType::Cidr,
            Self::Macaddr(_) => ExtendedType::Macaddr,
            Self::Macaddr8(_) => ExtendedType::Macaddr8,
            Self::PgLsn(_) => ExtendedType::PgLsn,
            Self::Bit(_) => ExtendedType::Bit { length: None },
            Self::VarBit(_) => ExtendedType::VarBit { length: None },
            Self::Date(_) => ExtendedType::Date,
            Self::Time(_) => ExtendedType::Time { precision: None },
            Self::TimeTz(_) => ExtendedType::TimeTz { precision: None },
            Self::Timestamp(_) => ExtendedType::Timestamp { precision: None },
            Self::TimestampTz(_) => ExtendedType::TimestampTz { precision: None },
            Self::Interval(_) => ExtendedType::Interval { typmod: -1 },
        }
    }

    /// `typoutput`: the canonical PostgreSQL text form.
    #[must_use]
    pub fn text(&self) -> String {
        self.text_in(temporal::default_session())
    }

    /// The schema type this value belongs to; the natively carried
    /// `date`/`time`/`timestamp` map back to their existing schema types.
    #[must_use]
    pub const fn data_type(&self) -> super::DataType {
        match self {
            Self::Date(_) => super::DataType::Date,
            Self::Time(_) => super::DataType::Time,
            Self::Timestamp(_) => super::DataType::Timestamp,
            other => super::DataType::Extended(other.kind()),
        }
    }

    /// The text output under a session's `TimeZone`, `DateStyle` and
    /// `IntervalStyle`.
    #[must_use]
    pub fn text_in(&self, session: &TemporalSession) -> String {
        use temporal::encode;
        match self {
            Self::Date(value) => encode::format_date(*value, session),
            Self::Time(value) => encode::format_time(*value),
            Self::TimeTz(value) => encode::format_timetz(value.time, value.zone),
            Self::Timestamp(value) => encode::format_timestamp(*value, session),
            Self::TimestampTz(value) => encode::format_timestamptz(*value, session),
            Self::Interval(value) => value.format(session.interval_style),
            Self::Real(value) => real::format_text(*value),
            Self::Numeric(value) => value.to_string(),
            Self::Inet(value) => network::format_network(*value, false),
            Self::Cidr(value) => network::format_network(*value, true),
            Self::Macaddr(value) => network::format_mac(value),
            Self::Macaddr8(value) => network::format_mac(value),
            Self::PgLsn(value) => pg_lsn::format_text(*value),
            Self::Bit(value) | Self::VarBit(value) => bit_string::format_text(value),
        }
    }

    /// The JSON carrier stored in documents; storage re-encodes it natively.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::Value::String(self.text())
    }

    #[must_use]
    pub fn key(&self) -> TypedKey {
        match self {
            Self::Real(value) => TypedKey::Real(real::order_key(*value)),
            Self::Numeric(value) => TypedKey::Numeric(value.order_key()),
            Self::Inet(value) | Self::Cidr(value) => TypedKey::Network(value.order_key()),
            Self::Macaddr(value) => TypedKey::Macaddr(*value),
            Self::Macaddr8(value) => TypedKey::Macaddr8(*value),
            Self::PgLsn(value) => TypedKey::PgLsn(*value),
            Self::Bit(value) | Self::VarBit(value) => {
                let (bytes, len) = value.order_key();
                TypedKey::Bits(bytes, len)
            }
            Self::Date(value) => TypedKey::Date(*value),
            Self::Time(value) => TypedKey::Time(*value),
            Self::TimeTz(value) => {
                let (time, zone) = value.order_key();
                TypedKey::TimeTz(time, zone)
            }
            Self::Timestamp(value) => TypedKey::Timestamp(*value),
            Self::TimestampTz(value) => TypedKey::TimestampTz(*value),
            Self::Interval(value) => TypedKey::Interval(value.order_key()),
        }
    }

    /// Heap bytes a clone retains, for query memory accounting.
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        match self {
            Self::Bit(value) | Self::VarBit(value) => value.heap_bytes(),
            Self::Numeric(numeric::Numeric::Finite { mantissa, .. }) => {
                usize::try_from(mantissa.bits().div_ceil(8)).unwrap_or(usize::MAX)
            }
            Self::Real(_)
            | Self::Numeric(_)
            | Self::Inet(_)
            | Self::Cidr(_)
            | Self::Macaddr(_)
            | Self::Macaddr8(_)
            | Self::PgLsn(_)
            | Self::Date(_)
            | Self::Time(_)
            | Self::TimeTz(_)
            | Self::Timestamp(_)
            | Self::TimestampTz(_)
            | Self::Interval(_) => 0,
        }
    }

    /// `typsend`: PostgreSQL binary output.
    #[must_use]
    pub fn send_binary(&self) -> Vec<u8> {
        match self {
            Self::Real(value) => value.to_be_bytes().to_vec(),
            Self::Numeric(value) => value.send_binary(),
            Self::Inet(value) => network::send_network(*value, false),
            Self::Cidr(value) => network::send_network(*value, true),
            Self::Macaddr(value) => value.to_vec(),
            Self::Macaddr8(value) => value.to_vec(),
            Self::PgLsn(value) => value.to_be_bytes().to_vec(),
            Self::Bit(value) | Self::VarBit(value) => bit_string::send_binary(value),
            Self::Date(value) => value.to_be_bytes().to_vec(),
            Self::Time(value) | Self::Timestamp(value) | Self::TimestampTz(value) => {
                value.to_be_bytes().to_vec()
            }
            Self::TimeTz(value) => temporal::send_timetz(value),
            Self::Interval(value) => temporal::send_interval(value),
        }
    }

    /// The value as `f64` when the family is numeric.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Real(value) => Some(f64::from(*value)),
            Self::Numeric(value) => Some(value.to_f64()),
            Self::Inet(_)
            | Self::Cidr(_)
            | Self::Macaddr(_)
            | Self::Macaddr8(_)
            | Self::PgLsn(_)
            | Self::Bit(_)
            | Self::VarBit(_)
            | Self::Date(_)
            | Self::Time(_)
            | Self::TimeTz(_)
            | Self::Timestamp(_)
            | Self::TimestampTz(_)
            | Self::Interval(_) => None,
        }
    }
}
