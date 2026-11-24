//! Finite existing-type `generate_series` overload resolution.
use crate::app::CassieError;
use crate::sql::ast::{Expr, FunctionCall};
use crate::types::{extended::ExtendedType, DataType};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Int4,
    Int8,
    Numeric,
    Timestamp,
    TimestampTz,
}

pub(crate) struct Signature {
    pub(crate) kind: Kind,
    pub(crate) inputs: Vec<DataType>,
    pub(crate) output: DataType,
}

pub(crate) fn resolve(
    function: &FunctionCall,
    mut argument_type: impl FnMut(&Expr) -> Option<DataType>,
) -> Result<Signature, CassieError> {
    let types = function
        .args
        .iter()
        .map(|argument| {
            if matches!(argument, Expr::Null | Expr::StringLiteral(_)) {
                None
            } else {
                argument_type(argument).filter(|kind| *kind != DataType::Null)
            }
        })
        .collect::<Vec<_>>();
    resolve_types(&types)
}

pub(crate) fn resolve_types(types: &[Option<DataType>]) -> Result<Signature, CassieError> {
    let count = types.len();
    if !(2..=4).contains(&count) {
        return Err(undefined());
    }
    let temporal = count == 4
        || types
            .iter()
            .flatten()
            .any(|kind| is_calendar(kind) || is_interval(kind));
    if temporal {
        if count < 3
            || types[..2].iter().flatten().any(|kind| !is_calendar(kind))
            || types[2].as_ref().is_some_and(|kind| !is_interval(kind))
            || (count == 4
                && types[3].as_ref().is_some_and(|kind| {
                    !matches!(
                        kind,
                        DataType::Text
                            | DataType::Name
                            | DataType::Varchar { .. }
                            | DataType::Char { .. }
                    )
                }))
        {
            return Err(undefined());
        }
        let tz = count == 4
            || types[..2].iter().flatten().any(is_tz)
            || !types[..2].iter().flatten().any(is_timestamp);
        let output = if tz {
            DataType::Extended(ExtendedType::TimestampTz { precision: None })
        } else {
            DataType::Timestamp
        };
        let mut inputs = vec![
            output.clone(),
            output.clone(),
            DataType::Extended(ExtendedType::Interval { typmod: -1 }),
        ];
        if count == 4 {
            inputs.push(DataType::Text);
        }
        return Ok(Signature {
            kind: if tz {
                Kind::TimestampTz
            } else {
                Kind::Timestamp
            },
            inputs,
            output,
        });
    }
    if count == 4
        || types.iter().flatten().any(|kind| {
            !matches!(
                kind,
                DataType::SmallInt
                    | DataType::Int
                    | DataType::BigInt
                    | DataType::Extended(ExtendedType::Numeric(_))
            )
        })
    {
        return Err(undefined());
    }
    if types.iter().all(Option::is_none) {
        return Err(CassieError::AmbiguousFunction(
            "function generate_series(unknown) is not unique".into(),
        ));
    }
    let (kind, output) = if types
        .iter()
        .flatten()
        .any(|kind| matches!(kind, DataType::Extended(ExtendedType::Numeric(_))))
    {
        (
            Kind::Numeric,
            DataType::Extended(ExtendedType::Numeric(None)),
        )
    } else if types.iter().flatten().any(|kind| *kind == DataType::BigInt) {
        (Kind::Int8, DataType::BigInt)
    } else {
        (Kind::Int4, DataType::Int)
    };
    Ok(Signature {
        kind,
        inputs: vec![output.clone(); count],
        output,
    })
}

pub(crate) fn undefined() -> CassieError {
    CassieError::Execution("function generate_series(...) does not exist".into())
}
fn is_timestamp(kind: &DataType) -> bool {
    matches!(
        kind,
        DataType::Timestamp | DataType::Extended(ExtendedType::Timestamp { .. })
    )
}
fn is_tz(kind: &DataType) -> bool {
    matches!(kind, DataType::Extended(ExtendedType::TimestampTz { .. }))
}
fn is_calendar(kind: &DataType) -> bool {
    is_timestamp(kind)
        || is_tz(kind)
        || matches!(
            kind,
            DataType::Date | DataType::Extended(ExtendedType::Date)
        )
}
fn is_interval(kind: &DataType) -> bool {
    matches!(kind, DataType::Extended(ExtendedType::Interval { .. }))
}
