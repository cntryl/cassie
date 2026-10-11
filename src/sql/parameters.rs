//! Parameter counts and current scalar inference; private implementation modules.

use crate::sql::{ast, ParsedStatement};
use crate::types::DataType;

mod count;
mod inference;

pub use count::parameter_count;
use inference::infer_parameter_type_oids_query;

const UNKNOWN_PARAMETER_TYPE_OID: i32 = 705;

struct ParameterInference {
    oids: Vec<i32>,
    context_demands: Vec<u8>,
    record_context_demands: bool,
}

impl ParameterInference {
    fn new(oids: Vec<i32>) -> Self {
        Self {
            context_demands: vec![0; oids.len()],
            oids,
            record_context_demands: true,
        }
    }

    fn is_empty(&self) -> bool {
        self.oids.is_empty()
    }

    fn record_type_demand(&mut self, index: usize, data_type: &DataType) {
        if !self.record_context_demands {
            return;
        }
        let demand = match data_type {
            DataType::Boolean => 1,
            DataType::SmallInt | DataType::Int | DataType::BigInt | DataType::Float => 2,
            _ => return,
        };
        if let Some(contexts) = self.context_demands.get_mut(index) {
            *contexts |= demand;
        }
    }

    fn record_expression_demand(&mut self, expr: &ast::Expr, data_type: &DataType) {
        if let ast::Expr::Param(index) = expr {
            self.record_type_demand(*index, data_type);
        }
    }

    fn without_context_demands(&mut self, infer: impl FnOnce(&mut Self)) {
        let previous = std::mem::replace(&mut self.record_context_demands, false);
        infer(self);
        self.record_context_demands = previous;
    }
}

#[must_use]
pub fn parameter_type_oids(statement: &ParsedStatement, provided: &[i32]) -> Vec<i32> {
    let count = parameter_count(statement);
    let mut oids = provided.iter().copied().take(count).collect::<Vec<_>>();
    if oids.len() < count {
        oids.extend(std::iter::repeat_n(
            UNKNOWN_PARAMETER_TYPE_OID,
            count - oids.len(),
        ));
    }
    oids
}

#[must_use]
pub fn parameter_type_oids_with_catalog(
    statement: &ParsedStatement,
    provided: &[i32],
    catalog: &crate::catalog::Catalog,
) -> Vec<i32> {
    infer_parameter_types_with_catalog(statement, provided, catalog).oids
}

fn infer_parameter_types_with_catalog(
    statement: &ParsedStatement,
    provided: &[i32],
    catalog: &crate::catalog::Catalog,
) -> ParameterInference {
    let mut inference = ParameterInference::new(parameter_type_oids(statement, provided));
    infer_parameter_type_oids_query(&statement.statement, catalog, &mut inference);
    inference
}

/// Returns a planner error message when a client-declared parameter type
/// cannot be compared with, or assigned to, the `BOOLEAN` operand it meets.
///
/// Literals of other families are rejected against `BOOLEAN` operands by
/// operand-family validation; a declared parameter type is held to the same
/// rule, as PostgreSQL does, instead of being compared by truthiness.
/// Explicit CAST result types do not create implicit Boolean input demands.
#[must_use]
pub fn declared_parameter_type_conflict(
    statement: &ParsedStatement,
    provided: &[i32],
    catalog: &crate::catalog::Catalog,
) -> Option<String> {
    let boolean_oid = i32::try_from(DataType::Boolean.type_oid()).ok()?;
    let inference = infer_parameter_types_with_catalog(statement, &[], catalog);
    if let Some(index) = inference
        .context_demands
        .iter()
        .position(|demand| *demand == 3)
    {
        return Some(format!(
            "parameter ${} has incompatible boolean and numeric contexts",
            index + 1
        ));
    }
    provided
        .iter()
        .zip(inference.context_demands)
        .find_map(|(declared, demand)| {
            if demand & 1 == 0
                || matches!(*declared, 0 | UNKNOWN_PARAMETER_TYPE_OID)
                || *declared == boolean_oid
            {
                return None;
            }
            let family = match *declared {
                20 | 21 | 23 | 700 | 701 | 1700 => "numeric",
                _ => "text",
            };
            Some(format!(
                "incompatible comparison operands: boolean and {family}"
            ))
        })
}
