//! Boolean aggregate transition and partial-state laws shared by row and typed paths.

use crate::executor::QueryError;
use crate::sql::functions::{self, FunctionId};
use crate::types::Value;

#[derive(Clone)]
pub(super) struct BooleanState {
    all: bool,
    value: Option<bool>,
}

impl BooleanState {
    pub(super) fn new(name: &str) -> Option<Self> {
        let all = match functions::function(name)?.id {
            FunctionId::BoolAnd => true,
            FunctionId::BoolOr => false,
            _ => return None,
        };
        Some(Self { all, value: None })
    }

    pub(super) fn empty_partial(&self) -> Self {
        Self {
            all: self.all,
            value: None,
        }
    }

    pub(super) fn update(&mut self, value: bool) {
        self.value = Some(self.value.map_or(value, |current| {
            if self.all {
                current && value
            } else {
                current || value
            }
        }));
    }

    pub(super) fn merge(&mut self, later: &Self) -> Result<(), QueryError> {
        if self.all != later.all {
            return Err(QueryError::General(
                "Boolean aggregate states have different operations".to_owned(),
            ));
        }
        if let Some(value) = later.value {
            self.update(value);
        }
        Ok(())
    }

    pub(super) fn finish(self) -> Value {
        self.value.map_or(Value::Null, Value::Bool)
    }
}
