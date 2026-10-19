//! Date/time builtins and their PostgreSQL result types.
use super::{FunctionArity, FunctionId, FunctionReturnType, ScalarFunction};
use crate::types::extended::ExtendedType;
use crate::types::DataType;

macro_rules! temporal_functions {
    ($($name:literal: $min:literal..=$max:literal),* $(,)?) => {
        pub(super) const FUNCTIONS: &[ScalarFunction] = &[
            $(ScalarFunction {
                id: FunctionId::Temporal,
                name: $name,
                arity: FunctionArity::Range { min: $min, max: $max },
                return_type: FunctionReturnType::Unknown,
                is_aggregate: false,
                bypass_arity_validation: false,
            },)*
        ];
    };
}

temporal_functions! {
    "now": 0..=0,
    "transaction_timestamp": 0..=0,
    "statement_timestamp": 0..=0,
    "clock_timestamp": 0..=0,
    "current_timestamp": 0..=1,
    "localtimestamp": 0..=1,
    "current_date": 0..=0,
    "current_time": 0..=1,
    "localtime": 0..=1,
    "timeofday": 0..=0,
    "date_trunc": 2..=3,
    "date_part": 2..=2,
    "extract": 2..=2,
    "date_bin": 3..=3,
    "age": 1..=2,
    "justify_days": 1..=1,
    "justify_hours": 1..=1,
    "justify_interval": 1..=1,
    "make_date": 3..=3,
    "make_time": 3..=3,
    "make_timestamp": 6..=6,
    "make_timestamptz": 6..=7,
    "make_interval": 0..=7,
    "to_timestamp": 1..=2,
    "to_date": 2..=2,
    "to_char": 2..=2,
    "isfinite": 1..=1,
    "overlaps": 4..=4,
    "timezone": 1..=2,
    "date_add": 2..=3,
    "date_subtract": 2..=3,
    "pg_sleep": 1..=1,
    "pg_sleep_for": 1..=1,
    "pg_sleep_until": 1..=1,
    "pg_conf_load_time": 0..=0,
    "to_number": 2..=2,
    "pg_postmaster_start_time": 0..=0,
}

const fn extended(kind: ExtendedType) -> DataType {
    DataType::Extended(kind)
}

const TIMESTAMPTZ: DataType = extended(ExtendedType::TimestampTz { precision: None });
const TIMETZ: DataType = extended(ExtendedType::TimeTz { precision: None });
const INTERVAL: DataType = extended(ExtendedType::Interval { typmod: -1 });

fn is_kind(data_type: Option<&DataType>, matches: impl Fn(&DataType) -> bool) -> bool {
    data_type.is_some_and(matches)
}

fn literal_precision(args: &[crate::sql::ast::Expr]) -> Option<u8> {
    match args.first() {
        Some(crate::sql::ast::Expr::IntegerLiteral(value)) => {
            u8::try_from((*value).clamp(0, 6)).ok()
        }
        _ => None,
    }
}

/// The result type of a date/time builtin given its argument types.
pub(crate) fn return_type(
    name: &str,
    args: &[crate::sql::ast::Expr],
    arg_type: &dyn Fn(usize) -> Option<DataType>,
) -> Option<DataType> {
    let name = name.strip_prefix("pg_catalog.").unwrap_or(name);
    let is_timestamp = |index| {
        is_kind(arg_type(index).as_ref(), |kind| {
            matches!(
                kind,
                DataType::Timestamp | DataType::Extended(ExtendedType::Timestamp { .. })
            )
        })
    };
    let is_interval = |index| {
        is_kind(arg_type(index).as_ref(), |kind| {
            matches!(kind, DataType::Extended(ExtendedType::Interval { .. }))
        })
    };
    let is_time = |index| {
        is_kind(arg_type(index).as_ref(), |kind| {
            matches!(
                kind,
                DataType::Time
                    | DataType::Extended(ExtendedType::Time { .. } | ExtendedType::TimeTz { .. })
            )
        })
    };
    Some(match name {
        "date_trunc" if is_interval(1) => INTERVAL,
        "sum" | "avg" if is_interval(0) => INTERVAL,
        "date_trunc" | "date_bin" if is_timestamp(1) => DataType::Timestamp,
        "date_trunc"
        | "date_bin"
        | "now"
        | "transaction_timestamp"
        | "statement_timestamp"
        | "clock_timestamp"
        | "to_timestamp"
        | "pg_conf_load_time"
        | "pg_postmaster_start_time"
        | "make_timestamptz"
        | "date_add"
        | "date_subtract" => TIMESTAMPTZ,
        "current_timestamp" => extended(ExtendedType::TimestampTz {
            precision: literal_precision(args),
        }),
        "localtimestamp" => match literal_precision(args) {
            Some(precision) => extended(ExtendedType::Timestamp {
                precision: Some(precision),
            }),
            None => DataType::Timestamp,
        },
        "current_time" => extended(ExtendedType::TimeTz {
            precision: literal_precision(args),
        }),
        "localtime" => match literal_precision(args) {
            Some(precision) => extended(ExtendedType::Time {
                precision: Some(precision),
            }),
            None => DataType::Time,
        },
        "current_date" | "make_date" | "to_date" => DataType::Date,
        "make_time" => DataType::Time,
        "make_timestamp" => DataType::Timestamp,
        "timeofday" | "to_char" => DataType::Text,
        "date_part" => DataType::Float,
        "extract" | "to_number" => extended(ExtendedType::Numeric(None)),
        "isfinite" | "overlaps" => DataType::Boolean,
        "age" | "justify_days" | "justify_hours" | "justify_interval" | "make_interval" => INTERVAL,
        "timezone" => {
            let value = args.len() - 1;
            if is_timestamp(value) {
                TIMESTAMPTZ
            } else if is_time(value) {
                TIMETZ
            } else {
                DataType::Timestamp
            }
        }
        _ => return None,
    })
}

const CLOCK_FUNCTIONS: [&str; 14] = [
    "now",
    "age",
    "transaction_timestamp",
    "statement_timestamp",
    "clock_timestamp",
    "current_timestamp",
    "localtimestamp",
    "current_date",
    "current_time",
    "localtime",
    "timeofday",
    "pg_sleep",
    "pg_sleep_for",
    "pg_sleep_until",
];

const CLOCK_INPUTS: [&str; 4] = ["now", "today", "tomorrow", "yesterday"];

/// Whether a serialized statement reads the clock, through a clock function
/// or a `'now'`/`'today'` style input, so its result must not be reused.
pub(crate) fn clock_dependent(encoded: &str) -> bool {
    let lower = encoded.to_ascii_lowercase();
    CLOCK_FUNCTIONS.iter().any(|name| {
        lower.contains(&format!("\"name\":\"{name}\""))
            || lower.contains(&format!("\"name\":\"pg_catalog.{name}\""))
    }) || CLOCK_INPUTS
        .iter()
        .any(|input| lower.contains(&format!("\"stringliteral\":\"{input}\"")))
}

/// Whether `name` is a date/time builtin.
pub(crate) fn is_temporal(name: &str) -> bool {
    super::function(name).is_some_and(|metadata| metadata.id == FunctionId::Temporal)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    Date,
    Time,
    TimeTz,
    Timestamp,
    TimestampTz,
    Interval,
    Integer,
    Number,
    Unknown,
}

fn shape(data_type: Option<&DataType>) -> Option<Shape> {
    Some(match data_type? {
        DataType::Date | DataType::Extended(ExtendedType::Date) => Shape::Date,
        DataType::Time | DataType::Extended(ExtendedType::Time { .. }) => Shape::Time,
        DataType::Extended(ExtendedType::TimeTz { .. }) => Shape::TimeTz,
        DataType::Timestamp | DataType::Extended(ExtendedType::Timestamp { .. }) => {
            Shape::Timestamp
        }
        DataType::Extended(ExtendedType::TimestampTz { .. }) => Shape::TimestampTz,
        DataType::Extended(ExtendedType::Interval { .. }) => Shape::Interval,
        DataType::SmallInt | DataType::Int | DataType::BigInt => Shape::Integer,
        DataType::Float | DataType::Extended(ExtendedType::Real | ExtendedType::Numeric(_)) => {
            Shape::Number
        }
        DataType::Text | DataType::Null => Shape::Unknown,
        _ => return None,
    })
}

/// The result type of `left op right` when either operand is temporal.
pub(crate) fn binary_type(
    op: &crate::sql::ast::BinaryOp,
    left: Option<&DataType>,
    right: Option<&DataType>,
) -> Option<DataType> {
    use crate::sql::ast::BinaryOp::{Add, Div, Mul, Sub};
    use Shape::{Date, Integer, Interval, Number, Time, TimeTz, Timestamp, TimestampTz, Unknown};
    let (left, right) = (shape(left)?, shape(right)?);
    let temporal = |shape| !matches!(shape, Integer | Number | Unknown);
    if !temporal(left) && !temporal(right) {
        return None;
    }
    let shape = match (left, op, right) {
        (Date, Add | Sub, Integer) | (Integer, Add, Date) => Date,
        (Date, Sub, Date) => return Some(DataType::Int),
        (Date | Timestamp, Add | Sub, Interval | Unknown)
        | (Interval | Time, Add, Date)
        | (Date, Add, Time)
        | (Interval, Add, Timestamp) => Timestamp,
        (Date, Add, TimeTz)
        | (TimeTz, Add, Date)
        | (TimestampTz, Add | Sub, Interval | Unknown)
        | (Interval, Add, TimestampTz) => TimestampTz,
        (Timestamp, Sub, Timestamp)
        | (TimestampTz, Sub, TimestampTz)
        | (Time, Sub, Time)
        | (Interval, Add | Sub, Interval | Unknown)
        | (Interval, Mul | Div, Integer | Number | Unknown)
        | (Integer | Number, Mul, Interval) => Interval,
        (Time, Add | Sub, Interval) | (Interval, Add, Time) => Time,
        (TimeTz, Add | Sub, Interval) | (Interval, Add, TimeTz) => TimeTz,
        _ => return None,
    };
    Some(match shape {
        Date => DataType::Date,
        Time => DataType::Time,
        TimeTz => TIMETZ,
        Timestamp => DataType::Timestamp,
        TimestampTz => TIMESTAMPTZ,
        _ => INTERVAL,
    })
}

/// Whether an aggregate accepts a temporal input (`sum`/`avg` of
/// `interval`; `min`/`max` of every temporal type).
pub(crate) fn aggregate_input(id: FunctionId, data_type: &DataType) -> bool {
    let DataType::Extended(kind) = data_type else {
        return false;
    };
    match id {
        FunctionId::Sum | FunctionId::Avg => matches!(kind, ExtendedType::Interval { .. }),
        FunctionId::Min | FunctionId::Max => matches!(
            kind,
            ExtendedType::Date
                | ExtendedType::Time { .. }
                | ExtendedType::TimeTz { .. }
                | ExtendedType::Timestamp { .. }
                | ExtendedType::TimestampTz { .. }
                | ExtendedType::Interval { .. }
        ),
        _ => false,
    }
}
