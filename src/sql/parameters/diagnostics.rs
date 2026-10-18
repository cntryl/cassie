//! Copied diagnostics for the qualified parameter-occurrence slice.

use std::fmt::{self, Write};

const PENDING_NAME_INPUT: &str = "pg_char_to_encoding NAME parameter identity remains pending under #793/#817; use an explicit TEXT argument";

const PENDING_DATUM_INPUT: &str = "direct pg_column_size parameter identity remains pending under #793/#817; use an explicit primitive cast";

const PENDING_TYPE_FUNCTION_INPUT: &str = "type-function overload input remains pending under #793/#796; use an explicitly typed argument";

const MAX_DECIMAL_DIGITS: usize = (usize::BITS as usize * 30_103) / 100_000 + 1;

#[derive(Clone, Copy, Debug)]
pub(crate) enum ScalarLiteralDiagnostic {
    Syntax(&'static str),
    Range(&'static str),
    FloatRange,
}

impl ScalarLiteralDiagnostic {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::Syntax(_) => "22P02",
            Self::Range(_) | Self::FloatRange => "22003",
        }
    }
    pub(crate) fn capacity(self, input: &str) -> Option<usize> {
        if matches!(self, Self::FloatRange) {
            return input
                .len()
                .checked_add("\"\" is out of range for type double precision".len());
        }
        let (name, overhead) = match self {
            Self::Syntax(name) => (name, "invalid input syntax for type : \"\"".len()),
            Self::Range(name) => (name, "value \"\" is out of range for type ".len()),
            Self::FloatRange => unreachable!("float capacity handled above"),
        };
        input.len().checked_add(name.len())?.checked_add(overhead)
    }
    pub(crate) fn write(self, input: &str, output: &mut impl Write) -> fmt::Result {
        match self {
            Self::Syntax(name) => {
                write!(output, "invalid input syntax for type {name}: \"{input}\"")
            }
            Self::Range(name) => {
                write!(output, "value \"{input}\" is out of range for type {name}")
            }
            Self::FloatRange => write!(
                output,
                "\"{input}\" is out of range for type double precision"
            ),
        }
    }
}

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
    Truth(crate::sql::ast::TruthValue, bool),
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
            Self::Truth(truth, negated) => truth.label(negated),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ParameterSemanticDiagnostic {
    DistinctAggregateOrder,
    ValuesWidth,
    ValuesTypes {
        left: i32,
        right: i32,
    },
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
    PendingNameInput,
    PendingDatumInput,
    PendingTypeFunctionInput,
    AmbiguousFunction {
        function: &'static str,
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
                | Self::ValuesTypes { .. }
                | Self::MissingEquality { .. }
                | Self::AmbiguousFunction { .. }
                | Self::UndefinedFunction { .. }
                | Self::BooleanOperand {
                    construct: BooleanConstruct::Filter | BooleanConstruct::Truth(..),
                    ..
                }
        )
    }
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::ValuesWidth => "42601",
            Self::PendingNameInput | Self::PendingDatumInput | Self::PendingTypeFunctionInput => {
                "0A000"
            }
            Self::AmbiguousFunction { .. } => "42725",
            Self::DistinctAggregateOrder => "42P10",
            Self::MissingEquality { .. } | Self::UndefinedFunction { .. } => "42883",
            Self::Inconsistent { .. } | Self::Indeterminate { .. } => "42P08",
            Self::Unresolved { .. } => "42P18",
            Self::ValuesTypes { .. } | Self::BooleanOperand { .. } => "42804",
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
            Self::ValuesWidth => "VALUES lists must all be the same length".len(),
            Self::PendingNameInput => PENDING_NAME_INPUT.len(),
            Self::PendingDatumInput => PENDING_DATUM_INPUT.len(),
            Self::PendingTypeFunctionInput => PENDING_TYPE_FUNCTION_INPUT.len(),
            Self::AmbiguousFunction { function } => {
                "function (unknown) is not unique".len() + function.len()
            }
            Self::ValuesTypes { left, right } => {
                "VALUES types  and  cannot be matched".len()
                    + type_name(left).len()
                    + type_name(right).len()
            }
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
            Self::PendingNameInput => output.write_str(PENDING_NAME_INPUT),
            Self::PendingDatumInput => output.write_str(PENDING_DATUM_INPUT),
            Self::PendingTypeFunctionInput => output.write_str(PENDING_TYPE_FUNCTION_INPUT),
            Self::ValuesWidth => output.write_str("VALUES lists must all be the same length"),
            Self::ValuesTypes { left, right } => write!(
                output,
                "VALUES types {} and {} cannot be matched",
                type_name(left),
                type_name(right)
            ),
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
            Self::AmbiguousFunction { function } => {
                write!(output, "function {function}(unknown) is not unique")
            }
            Self::UndefinedFunction {
                function,
                argument_count,
                actual,
            } => {
                write!(output, "function {function}(")?;
                if let Some(oid) = actual {
                    write!(output, "{}", type_name(oid))?;
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
            | Self::AmbiguousFunction { .. }
            | Self::PendingNameInput
            | Self::PendingDatumInput
            | Self::PendingTypeFunctionInput
            | Self::DistinctAggregateOrder
            | Self::ValuesTypes { .. }
            | Self::ValuesWidth
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

#[derive(Clone, Copy)]
pub(super) enum TypeName {
    Scalar(&'static str),
    Vector(usize),
}

impl TypeName {
    pub(super) fn len(self) -> usize {
        match self {
            Self::Scalar(name) => name.len(),
            Self::Vector(_) => "vector()".len() + MAX_DECIMAL_DIGITS,
        }
    }
}

impl fmt::Display for TypeName {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scalar(name) => output.write_str(name),
            Self::Vector(dimensions) => write!(output, "vector({dimensions})"),
        }
    }
}

pub(super) fn type_name(oid: i32) -> TypeName {
    if let Some(dimensions) = crate::types::schema::vector_dimensions_for_oid(i64::from(oid)) {
        return TypeName::Vector(dimensions);
    }
    TypeName::Scalar(match oid {
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
    })
}
