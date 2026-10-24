//! Mergeable transitions for the integer bitwise aggregate family.

use crate::sql::ast::FunctionCall;
use crate::sql::functions::{self, FunctionId};
use crate::types::Value;

use super::QueryError;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Operation {
    And,
    Or,
    Xor,
}

#[derive(Clone)]
pub(super) struct BitwiseState {
    operation: Operation,
    value: Option<i64>,
}

impl BitwiseState {
    pub(super) fn new(function: &FunctionCall) -> Option<Self> {
        let operation = match functions::function(&function.name)?.id {
            FunctionId::BitAnd => Operation::And,
            FunctionId::BitOr => Operation::Or,
            FunctionId::BitXor => Operation::Xor,
            _ => return None,
        };
        Some(Self {
            operation,
            value: None,
        })
    }

    pub(super) fn update(
        &mut self,
        function: &FunctionCall,
        value: Value,
    ) -> Result<(), QueryError> {
        match value {
            Value::Null => {}
            Value::Int64(value) => {
                self.value = Some(
                    self.value
                        .map_or(value, |current| self.operation.combine(current, value)),
                );
            }
            value => {
                return Err(QueryError::General(format!(
                    "{} aggregate received non-integer input {value:?}",
                    function.name
                )));
            }
        }
        Ok(())
    }

    pub(super) fn merge(&mut self, later: &Self) -> Result<(), QueryError> {
        if self.operation != later.operation {
            return Err(QueryError::General(
                "bitwise aggregate states have different operations".to_owned(),
            ));
        }
        if let Some(value) = later.value {
            self.value = Some(
                self.value
                    .map_or(value, |current| self.operation.combine(current, value)),
            );
        }
        Ok(())
    }

    pub(super) fn finish(self) -> Value {
        self.value.map_or(Value::Null, Value::Int64)
    }
}

impl Operation {
    fn combine(self, left: i64, right: i64) -> i64 {
        match self {
            Self::And => left & right,
            Self::Or => left | right,
            Self::Xor => left ^ right,
        }
    }
}
