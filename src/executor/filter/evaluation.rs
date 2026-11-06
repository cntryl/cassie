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
    pub(crate) fn grow(&self, bytes: usize) -> Result<(), QueryError> {
        self.0.borrow_mut().try_grow(bytes).map_err(Into::into)
    }
    pub(crate) fn into_reservation(self) -> QueryMemoryReservation {
        self.0.into_inner()
    }
}

/// Admit copied scalar leaves before allocation. The owner spans the complete
/// expression, including nested function arguments and enclosing pattern work.
pub(super) fn admit_leaf<R: RowAccess + ?Sized>(
    row: &R,
    expr: &Expr,
    context: EvalContext<'_>,
) -> Result<(), QueryError> {
    let Some(memory) = context.memory else {
        return Ok(());
    };
    if let Some(controls) = context.controls {
        controls.check_interruption()?;
    }
    let bytes = match expr {
        Expr::StringLiteral(text) => text.len(),
        Expr::NumericLiteral(text) => super::cast_admission::numeric_input_bytes(text)?,
        Expr::Column(name) => super::column_value_ref(row, name, context.local_args)
            .map(super::cast_admission::value_workspace_bytes)
            .transpose()?
            .unwrap_or(0),
        Expr::Param(index) => context
            .params
            .get(*index)
            .map(super::cast_admission::value_workspace_bytes)
            .transpose()?
            .unwrap_or(0),
        _ => 0,
    };
    memory.grow(crate::executor::retained_memory::mul(bytes, 4)?)
}

pub(crate) struct ProjectionValueContext<'a> {
    pub(crate) params: &'a [Value],
    pub(crate) search: Option<&'a SearchContext>,
    pub(crate) functions: &'a HashMap<String, FunctionMeta>,
    pub(crate) session: Option<&'a CassieSession>,
    pub(crate) resolver: Option<&'a super::ExistsResolver<'a>>,
}

pub(crate) fn evaluate_projection_value<R: RowAccess + ?Sized>(
    row: &R,
    expression: &Expr,
    context: &ProjectionValueContext<'_>,
    controls: &QueryExecutionControls,
    memory: &EvaluationMemory,
) -> Result<Value, QueryError> {
    controls.check_interruption()?;
    evaluate_expr_value_with_context(
        row,
        expression,
        EvalContext {
            params: context.params,
            search_context: context.search,
            user_functions: context.functions,
            local_args: None,
            session: context.session,
            exists: context.resolver,
            controls: Some(controls),
            memory: Some(memory),
        },
    )
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
    #[test]
    fn should_admit_reached_concatenation_leaves_before_copies() {
        // Arrange
        let limits = crate::config::CassieRuntimeLimits {
            query_memory_budget_bytes: 0,
            ..Default::default()
        };
        let controls = QueryExecutionControls::from_limits(&limits, std::time::Instant::now());
        let memory = EvaluationMemory::new(&controls).expect("empty owner");
        let row = crate::executor::batch::BatchRow::new(vec![(
            "input".into(),
            Value::String("bounded".into()),
        )]);
        let expr = crate::sql::parser::parse_expression("input || 'tail'").expect("expression");
        // Act
        let result =
            evaluate_source_argument(&row, &expr, &[], &HashMap::new(), None, &controls, &memory);
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
    #[test]
    fn should_reject_unadmitted_expanding_casts_before_producing_values() {
        // Arrange
        for (sql, input) in [
            ("CAST(input AS CHAR(512))", "x"),
            ("CAST(input AS JSONB)", "1e2048"),
            ("CAST(input AS NUMERIC)", "1e2_048"),
        ] {
            let controls = QueryExecutionControls::from_limits(
                &crate::config::CassieRuntimeLimits {
                    query_memory_budget_bytes: 1024,
                    ..Default::default()
                },
                Instant::now(),
            );
            let memory = EvaluationMemory::new(&controls).expect("owner");
            let row = crate::executor::batch::BatchRow::new(vec![(
                "input".into(),
                Value::String(input.into()),
            )]);
            let expression = crate::sql::parser::parse_expression(sql).expect("expanding cast");
            // Act
            let result = evaluate_source_argument(
                &row,
                &expression,
                &[],
                &HashMap::new(),
                None,
                &controls,
                &memory,
            );
            // Assert
            assert!(
                matches!(
                    result,
                    Err(QueryError::Cassie(crate::app::CassieError::ResourceLimit(
                        _
                    )))
                ),
                "{sql}: expected resource denial"
            );
            drop(result);
            drop(memory);
            assert_eq!(controls.current_query_memory_bytes(), 0);
        }
    }

    #[test]
    fn should_skip_unreached_expanding_casts() {
        // Arrange
        let controls = QueryExecutionControls::from_limits(
            &crate::config::CassieRuntimeLimits {
                query_memory_budget_bytes: 1024,
                ..Default::default()
            },
            Instant::now(),
        );
        let memory = EvaluationMemory::new(&controls).expect("owner");
        let row = crate::executor::batch::BatchRow::from_projected_values(vec![]);
        let expression = crate::sql::parser::parse_expression(
            "CASE WHEN FALSE THEN CAST('x' AS CHAR(512)) ELSE 'ok' END",
        )
        .expect("lazy cast");
        // Act
        let result = evaluate_source_argument(
            &row,
            &expression,
            &[],
            &HashMap::new(),
            None,
            &controls,
            &memory,
        )
        .expect("unreached cast");
        // Assert
        assert_eq!(result, Value::String("ok".into()));
    }
    #[test]
    fn should_admit_escaped_json_text_before_scalar_serialization() {
        // Arrange
        let document = serde_json::Value::String("\0".repeat(100));
        let row = crate::executor::batch::BatchRow::new(vec![(
            "input".into(),
            Value::Json(document.clone()),
        )]);
        let expression = Expr::Cast {
            expr: Box::new(Expr::Column("input".into())),
            data_type: crate::types::DataType::Text,
            source_type: Some(crate::types::DataType::Json),
            identity: crate::sql::ast::CastIdentity::Scalar,
        };
        for (budget, accepted) in [(800, false), (4096, true)] {
            let controls = QueryExecutionControls::from_limits(
                &crate::config::CassieRuntimeLimits {
                    query_memory_budget_bytes: budget,
                    ..Default::default()
                },
                Instant::now(),
            );
            let memory = EvaluationMemory::new(&controls).expect("owner");
            // Act
            let result = evaluate_source_argument(
                &row,
                &expression,
                &[],
                &HashMap::new(),
                None,
                &controls,
                &memory,
            );
            // Assert
            if accepted {
                assert_eq!(
                    result.as_ref().expect("admitted JSON text"),
                    &Value::String(serde_json::to_string(&document).expect("JSON text"))
                );
            } else {
                assert!(matches!(
                    &result,
                    Err(QueryError::Cassie(crate::app::CassieError::ResourceLimit(
                        _
                    )))
                ));
            }
            drop(result);
            drop(memory);
            assert_eq!(controls.current_query_memory_bytes(), 0);
        }
    }
}
