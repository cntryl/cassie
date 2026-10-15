//! Copied diagnostics for the qualified parameter-occurrence slice.

use std::fmt::{self, Write};

const MAX_DECIMAL_DIGITS: usize = (usize::BITS as usize * 30_103) / 100_000 + 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BooleanConstruct {
    Where,
    Having,
    Join,
    Not,
    And,
    Or,
    CaseWhen,
    Filter,
}

impl BooleanConstruct {
    const fn name(self) -> &'static str {
        match self {
            Self::Where => "WHERE",
            Self::Having => "HAVING",
            Self::Join => "JOIN/ON",
            Self::Not => "NOT",
            Self::And => "AND",
            Self::Or => "OR",
            Self::CaseWhen => "CASE/WHEN",
            Self::Filter => "FILTER",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ParameterSemanticDiagnostic {
    DistinctAggregateOrder,
    MissingEquality {
        actual: i32,
    },
    Inconsistent {
        slot: usize,
        previous: i32,
        requested: i32,
    },
    Indeterminate {
        slot: usize,
    },
    Unresolved {
        slot: usize,
    },
    BooleanOperand {
        construct: BooleanConstruct,
        actual: i32,
    },
    UndefinedFunction {
        function: &'static str,
        argument_count: usize,
        actual: Option<i32>,
    },
}

impl ParameterSemanticDiagnostic {
    pub(crate) fn uses_execution_error(self) -> bool {
        matches!(
            self,
            Self::DistinctAggregateOrder
                | Self::MissingEquality { .. }
                | Self::UndefinedFunction { .. }
                | Self::BooleanOperand {
                    construct: BooleanConstruct::Filter,
                    ..
                }
        )
    }
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::DistinctAggregateOrder => "42P10",
            Self::MissingEquality { .. } | Self::UndefinedFunction { .. } => "42883",
            Self::Inconsistent { .. } | Self::Indeterminate { .. } => "42P08",
            Self::Unresolved { .. } => "42P18",
            Self::BooleanOperand { .. } => "42804",
        }
    }

    pub(crate) fn message(self) -> String {
        let mut message = String::with_capacity(self.message_capacity());
        self.write_message(&mut message)
            .expect("String formatting cannot fail");
        message
    }

    pub(crate) fn message_capacity(self) -> usize {
        match self {
            Self::DistinctAggregateOrder => {
                "in an aggregate with DISTINCT, ORDER BY expressions must appear in argument list"
                    .len()
            }
            Self::MissingEquality { actual } => {
                "could not identify an equality operator for type ".len() + type_name(actual).len()
            }
            Self::Inconsistent { .. } => {
                "inconsistent types deduced for parameter $".len() + MAX_DECIMAL_DIGITS
            }
            Self::Indeterminate { .. } | Self::Unresolved { .. } => {
                "could not determine data type of parameter $".len() + MAX_DECIMAL_DIGITS
            }
            Self::BooleanOperand { construct, actual } => {
                "argument of ".len()
                    + construct.name().len()
                    + " must be type boolean, not type ".len()
                    + type_name(actual).len()
            }
            Self::UndefinedFunction {
                function,
                argument_count,
                actual,
            } => {
                "function () does not exist".len()
                    + function.len()
                    + actual.map_or_else(
                        || if argument_count == 0 { 0 } else { 3 },
                        |oid| type_name(oid).len(),
                    )
            }
        }
    }

    pub(crate) fn write_message(self, output: &mut impl Write) -> fmt::Result {
        match self {
            Self::DistinctAggregateOrder => output.write_str(
                "in an aggregate with DISTINCT, ORDER BY expressions must appear in argument list",
            ),
            Self::MissingEquality { actual } => write!(
                output,
                "could not identify an equality operator for type {}",
                type_name(actual)
            ),
            Self::Inconsistent { slot, .. } => {
                write!(
                    output,
                    "inconsistent types deduced for parameter ${}",
                    slot.saturating_add(1)
                )
            }
            Self::Indeterminate { slot } | Self::Unresolved { slot } => {
                write!(
                    output,
                    "could not determine data type of parameter ${}",
                    slot.saturating_add(1)
                )
            }
            Self::BooleanOperand { construct, actual } => write!(
                output,
                "argument of {} must be type boolean, not type {}",
                construct.name(),
                type_name(actual)
            ),
            Self::UndefinedFunction {
                function,
                argument_count,
                actual,
            } => {
                write!(output, "function {function}(")?;
                if let Some(oid) = actual {
                    output.write_str(type_name(oid))?;
                } else if argument_count != 0 {
                    output.write_str("...")?;
                }
                output.write_str(") does not exist")
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn detail(self) -> Option<String> {
        let mut detail = String::with_capacity(self.detail_capacity()?);
        self.write_detail(&mut detail)
            .expect("String formatting cannot fail");
        Some(detail)
    }

    pub(crate) fn detail_capacity(self) -> Option<usize> {
        match self {
            Self::Inconsistent {
                previous,
                requested,
                ..
            } => Some(type_name(previous).len() + " versus ".len() + type_name(requested).len()),
            Self::Indeterminate { .. }
            | Self::Unresolved { .. }
            | Self::BooleanOperand { .. }
            | Self::UndefinedFunction { .. }
            | Self::DistinctAggregateOrder
            | Self::MissingEquality { .. } => None,
        }
    }

    pub(crate) fn write_detail(self, output: &mut impl Write) -> fmt::Result {
        if let Self::Inconsistent {
            previous,
            requested,
            ..
        } = self
        {
            write!(
                output,
                "{} versus {}",
                type_name(previous),
                type_name(requested)
            )?;
        }
        Ok(())
    }
}

fn type_name(oid: i32) -> &'static str {
    match oid {
        16 => "boolean",
        20 => "bigint",
        21 => "smallint",
        23 => "integer",
        25 => "text",
        1042 => "character",
        1043 => "character varying",
        700 => "real",
        701 => "double precision",
        1700 => "numeric",
        114 => "json",
        2950 => "uuid",
        17 => "bytea",
        1082 => "date",
        1083 => "time without time zone",
        1114 => "timestamp without time zone",
        _ => "unknown",
    }
}
