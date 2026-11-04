//! Retain data-dependent character-array cast scratch through projected rows.
use super::super::filter::evaluation::{evaluate_projection_value, ProjectionValueContext};
use super::super::filter::EvaluationMemory;
use super::{BatchRow, CassieSession, ProjectionOp, QueryError, RowAccess};
use crate::runtime::QueryExecutionControls;
use crate::types::DataType;

pub(super) fn required(ops: &[ProjectionOp]) -> bool {
    ops.iter().any(|op| match op {
        ProjectionOp::ScalarFunction { expr, .. } => expr.any_descendant_or_self(&mut |expr| {
            matches!(expr, crate::sql::ast::Expr::Cast { data_type: DataType::Array(element), .. }
                if matches!(element.as_ref(), DataType::Text | DataType::Char { .. } | DataType::Varchar { .. }))
        }),
        _ => false,
    })
}

pub(super) struct Memory<'a> {
    controls: Option<&'a QueryExecutionControls>,
    memory: Option<EvaluationMemory>,
}
impl<'a> Memory<'a> {
    pub(super) fn new(
        required: bool,
        session: Option<&'a CassieSession>,
    ) -> Result<Self, QueryError> {
        let controls = session
            .filter(|_| required)
            .and_then(CassieSession::statement_execution_controls);
        Ok(Self {
            controls,
            memory: controls.map(EvaluationMemory::new).transpose()?,
        })
    }
    pub(super) fn evaluate<R: RowAccess + ?Sized>(
        &self,
        row: &R,
        expr: &crate::sql::ast::Expr,
        context: &ProjectionValueContext<'_>,
    ) -> Option<Result<crate::types::Value, QueryError>> {
        Some(evaluate_projection_value(
            row,
            expr,
            context,
            self.controls?,
            self.memory.as_ref()?,
        ))
    }
    pub(super) fn retain(self, row: BatchRow) -> Result<BatchRow, QueryError> {
        let (Some(controls), Some(memory)) = (self.controls, self.memory) else {
            return Ok(row);
        };
        memory.grow(
            std::mem::size_of::<crate::runtime::QueryMemoryReservation>()
                + 2 * std::mem::size_of::<usize>(),
        )?;
        row.retain_operator_memory(controls, std::sync::Arc::new(memory.into_reservation()))
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{SqlArray, Value};
    use std::time::Instant;

    #[test]
    fn should_keep_character_array_projection_padding_owned_after_expression_evaluation() {
        // Arrange
        let controls = QueryExecutionControls::from_limits(
            &crate::config::CassieRuntimeLimits::default(),
            Instant::now(),
        );
        let memory = Memory {
            controls: Some(&controls),
            memory: Some(EvaluationMemory::new(&controls).expect("projection owner")),
        };
        let params = [Value::Array(
            SqlArray::from_elements(vec![Value::String("é".into())]).expect("native array"),
        )];
        let functions = std::collections::HashMap::new();
        let context = ProjectionValueContext {
            params: &params,
            search: None,
            functions: &functions,
            session: None,
            resolver: None,
        };
        let expression = crate::sql::parser::parse_expression("$1::CHAR(3)[]").expect("array cast");
        let input = BatchRow::from_projected_values(Vec::new());

        // Act
        let value = memory
            .evaluate(&input, &expression, &context)
            .expect("controlled projection")
            .expect("character array");
        let row = memory
            .retain(BatchRow::new(vec![("items".into(), value)]))
            .expect("retained projection");
        let retained = controls.current_query_memory_bytes();

        // Assert
        let Some(Value::Array(array)) = row.get("items") else {
            panic!("native array result");
        };
        assert_eq!(array.elements(), &[Value::String("é  ".into())]);
        assert!(retained > 0);
        drop(row);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
