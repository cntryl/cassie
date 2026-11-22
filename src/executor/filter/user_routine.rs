//! SQL routine argument and body evaluation under the invoking expression context.
use super::{
    cast_scalar, eval_scalar, execution_controls, merge_local_args, scalar_from_value, EvalContext,
    EvaluationMemory, HashMap, QueryError, RowAccess, Value,
};
use std::cell::Cell;

const MAX_USER_FUNCTION_DEPTH: usize = 64;

thread_local! {
    static USER_FUNCTION_DEPTH: Cell<usize> = const { Cell::new(0) };
}

struct UserFunctionDepthGuard(usize);

struct RoutineMemoryGuard<'a> {
    memory: &'a EvaluationMemory,
    baseline: usize,
}

impl Drop for RoutineMemoryGuard<'_> {
    fn drop(&mut self) {
        self.memory.shrink_to(self.baseline);
    }
}

impl Drop for UserFunctionDepthGuard {
    fn drop(&mut self) {
        USER_FUNCTION_DEPTH.with(|depth| depth.set(self.0));
    }
}

fn enter_user_function_depth() -> Result<UserFunctionDepthGuard, QueryError> {
    USER_FUNCTION_DEPTH.with(|depth| {
        let previous = depth.get();
        if previous >= MAX_USER_FUNCTION_DEPTH {
            return Err(crate::app::CassieError::from(
                crate::app::DiagnosticError::new(
                    "54001",
                    "stack depth limit exceeded",
                )
                .with_hint("Increase the configuration parameter max_stack_depth"),
            )
            .into());
        }
        depth.set(previous + 1);
        Ok(UserFunctionDepthGuard(previous))
    })
}

pub(super) fn evaluate_user_defined_function<R: RowAccess + ?Sized>(
    name: &str,
    row: &R,
    context: EvalContext<'_>,
    args: Vec<Value>,
) -> Result<Value, QueryError> {
    let _depth = enter_user_function_depth()?;
    let lookup = name.to_ascii_lowercase();
    let Some(metadata) = context
        .user_functions
        .get(name)
        .or_else(|| context.user_functions.get(&lookup))
    else {
        return Err(QueryError::General(format!(
            "unsupported function '{name}'"
        )));
    };
    if args.len() != metadata.args.len() {
        return Err(QueryError::General(format!(
            "function '{name}' expects {} args, got {}",
            metadata.args.len(),
            args.len()
        )));
    }
    let baseline = context.memory.map(EvaluationMemory::bytes);
    let _routine_memory = baseline.zip(context.memory).map(|(baseline, memory)| {
        RoutineMemoryGuard { memory, baseline }
    });
    let _function_memory_scope = if let Some(memory) = context.memory {
        Some(memory.scope(crate::executor::retained_memory::add(
            crate::executor::retained_memory::mul(metadata.body.len(), 128)?,
            1024,
        )?)?)
    } else {
        None
    };
    let body = crate::sql::parser::parse_expression(&metadata.body).map_err(|error| {
        QueryError::General(format!("invalid function body for '{name}': {error}"))
    })?;
    crate::sql::definition_guard::legacy_expression(&body).map_err(QueryError::Cassie)?;
    let locals = metadata
        .args
        .iter()
        .zip(args)
        .map(|(arg, value)| {
            let value = cast_scalar(
                &scalar_from_value(&value),
                &arg.data_type,
                execution_controls(context),
            )?;
            Ok((arg.name.to_ascii_lowercase(), value.to_value()))
        })
        .collect::<Result<HashMap<String, Value>, QueryError>>()?;
    let merged_args = merge_local_args(context.local_args, locals);
    if context.memory.is_some() {
        return super::super::evaluate_expr_value_with_context(
            row,
            &body,
            EvalContext {
                local_args: Some(&merged_args),
                ..context
            },
        )
        .and_then(|value| {
            cast_scalar(
                &scalar_from_value(&value),
                &metadata.return_type,
                execution_controls(context),
            )
        })
        .map(|value| value.to_value());
    }
    eval_scalar(
        row,
        &body,
        context.params,
        context.search_context,
        context.user_functions,
        Some(&merged_args),
        context.session,
    )
    .and_then(|value| cast_scalar(&value, &metadata.return_type, execution_controls(context)))
    .map(|value| value.to_value())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{FunctionArgMeta, FunctionMeta, Volatility};
    use crate::config::CassieRuntimeLimits;
    use crate::executor::batch::BatchRow;
    use crate::executor::filter::{evaluate_expr_value_with_context, EvaluationMemory};
    use crate::runtime::QueryExecutionControls;
    use crate::sql::parser::parse_expression;
    use crate::types::DataType;
    use std::{collections::HashMap, time::Instant};

    fn recursive_functions() -> HashMap<String, FunctionMeta> {
        [
            (
                "rm_f".to_owned(),
                FunctionMeta {
                    name: "rm_f".to_owned(),
                    args: vec![FunctionArgMeta {
                        name: "a".to_owned(),
                        data_type: DataType::Int,
                    }],
                    return_type: DataType::Int,
                    volatility: Volatility::Immutable,
                    body: "rm_g(a)".to_owned(),
                    object_id: 1,
                },
            ),
            (
                "rm_g".to_owned(),
                FunctionMeta {
                    name: "rm_g".to_owned(),
                    args: vec![FunctionArgMeta {
                        name: "a".to_owned(),
                        data_type: DataType::Int,
                    }],
                    return_type: DataType::Int,
                    volatility: Volatility::Immutable,
                    body: "rm_f(a)".to_owned(),
                    object_id: 2,
                },
            ),
        ]
        .into_iter()
        .collect()
    }

    #[test]
    fn should_reject_mutual_function_recursion_with_sqlstate_54001_without_memory() {
        // Arrange
        let controls = QueryExecutionControls::from_limits(
            &CassieRuntimeLimits::default(),
            Instant::now(),
        );
        let functions = recursive_functions();
        let row = BatchRow::new(Vec::new());
        let expression = parse_expression("rm_f(1)").expect("call parses");
        let context = EvalContext {
            params: &[],
            search_context: None,
            user_functions: &functions,
            local_args: None,
            session: None,
            exists: None,
            controls: Some(&controls),
            memory: None,
            deletion_memory: None,
        };

        // Act
        let error = evaluate_expr_value_with_context(&row, &expression, context)
            .expect_err("recursive call must exceed logical depth");

        // Assert
        assert!(matches!(
            error,
            QueryError::Cassie(crate::app::CassieError::Diagnostic(ref diagnostic))
                if diagnostic.sql_state == "54001"
                    && diagnostic.message == "stack depth limit exceeded"
        ));
    }

    #[test]
    fn should_reject_mutual_function_recursion_with_sqlstate_54001_with_memory() {
        // Arrange
        let controls = QueryExecutionControls::from_limits(
            &CassieRuntimeLimits::default(),
            Instant::now(),
        );
        let memory = EvaluationMemory::new(&controls).expect("memory");
        let functions = recursive_functions();
        let row = BatchRow::new(Vec::new());
        let expression = parse_expression("rm_f(1)").expect("call parses");
        let context = EvalContext {
            params: &[],
            search_context: None,
            user_functions: &functions,
            local_args: None,
            session: None,
            exists: None,
            controls: Some(&controls),
            memory: Some(&memory),
            deletion_memory: None,
        };

        // Act
        let error = evaluate_expr_value_with_context(&row, &expression, context)
            .expect_err("recursive call must exceed logical depth");

        // Assert
        assert!(matches!(
            error,
            QueryError::Cassie(crate::app::CassieError::Diagnostic(ref diagnostic))
                if diagnostic.sql_state == "54001"
                    && diagnostic.message == "stack depth limit exceeded"
        ));
        assert_eq!(memory.bytes(), 0);
    }
}
