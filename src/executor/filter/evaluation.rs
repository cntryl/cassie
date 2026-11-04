//! Scalar entry points and private ownership for generated-source arguments.
use super::{
    evaluate_expr_value_with_context, CassieSession, EvalContext, Expr, FunctionMeta, HashMap,
    QueryError, RowAccess, SearchContext, Value,
};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use std::cell::RefCell;

pub(crate) struct EvaluationMemory(RefCell<QueryMemoryReservation>);

impl EvaluationMemory {
    pub(crate) fn new(controls: &QueryExecutionControls) -> Result<Self, QueryError> {
        Ok(Self(RefCell::new(controls.reserve_query_memory(0)?)))
    }
    pub(super) fn grow(&self, bytes: usize) -> Result<(), QueryError> {
        self.0.borrow_mut().try_grow(bytes).map_err(Into::into)
    }
}

pub(crate) fn evaluate_source_argument<R: RowAccess + ?Sized>(
    row: &R,
    expression: &Expr,
    params: &[Value],
    functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
    controls: &QueryExecutionControls,
    memory: &EvaluationMemory,
) -> Result<Value, QueryError> {
    controls.check_interruption()?;
    super::evaluate_expr_value_with_context(
        row,
        expression,
        EvalContext {
            params,
            search_context: None,
            user_functions: functions,
            local_args: None,
            session,
            exists: None,
            controls: Some(controls),
            memory: Some(memory),
        },
    )
}

pub(crate) fn evaluate_source_argument_resolving<R: RowAccess + ?Sized>(
    row: &R,
    expression: &Expr,
    context: super::ExistsValueContext<'_>,
    controls: &QueryExecutionControls,
    memory: &EvaluationMemory,
) -> Result<Value, QueryError> {
    controls.check_interruption()?;
    super::evaluate_expr_value_with_context(
        row,
        expression,
        EvalContext {
            params: context.params,
            search_context: context.search,
            user_functions: context.functions,
            local_args: None,
            session: context.session,
            exists: Some(context.resolver),
            controls: Some(controls),
            memory: Some(memory),
        },
    )
}

pub(crate) fn evaluate_expr_value<R: RowAccess + ?Sized>(
    row: &R,
    expr: &Expr,
    params: &[Value],
    search_context: Option<&SearchContext>,
    user_functions: &HashMap<String, FunctionMeta>,
    session: Option<&CassieSession>,
    local_args: Option<&HashMap<String, Value>>,
) -> Result<Value, QueryError> {
    evaluate_expr_value_with_context(
        row,
        expr,
        EvalContext {
            params,
            search_context,
            user_functions,
            local_args,
            session,
            exists: None,
            controls: None,
            memory: None,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn should_deny_computed_array_arguments_before_retaining_unadmitted_fill_output() {
        // Arrange
        let controls = QueryExecutionControls::from_limits(
            &crate::config::CassieRuntimeLimits {
                query_memory_budget_bytes: 16_384,
                ..crate::config::CassieRuntimeLimits::default()
            },
            Instant::now(),
        );
        let memory = EvaluationMemory::new(&controls).expect("expression owner");
        let expression = crate::sql::parser::parse_expression("array_fill(1, ARRAY[512])")
            .expect("computed array");
        let functions = HashMap::new();
        let row = crate::executor::batch::BatchRow::from_projected_values(Vec::new());

        // Act
        let result =
            evaluate_source_argument(&row, &expression, &[], &functions, None, &controls, &memory);

        // Assert
        assert!(matches!(
            result,
            Err(QueryError::Cassie(crate::app::CassieError::ResourceLimit(
                _
            )))
        ));
        drop(result);
        drop(memory);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
