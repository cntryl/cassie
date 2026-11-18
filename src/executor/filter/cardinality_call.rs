//! Native counting owns its argument copies without decimal formatting scratch.

use super::{EvalContext, FunctionCall, QueryError, RowAccess, Value};
use crate::executor::filter::EvaluationMemory;
use crate::executor::retained_memory::{add, grown_capacity, mul, value_clone_bytes};
use crate::sql::ast::Expr;

pub(super) fn evaluate<R: RowAccess + ?Sized>(
    function: &FunctionCall,
    row: &R,
    context: EvalContext<'_>,
) -> Result<Value, QueryError> {
    check(context)?;
    let outer = context.memory.or(context.deletion_memory);
    let local = if outer.is_none() {
        context.controls.map(EvaluationMemory::new).transpose()?
    } else {
        None
    };
    let context = EvalContext {
        memory: outer.or(local.as_ref()),
        ..context
    };
    // Reserve the argument directory before its first allocation. Its owner
    // predates args and therefore drops after args on every return/error path.
    let directory = mul(
        grown_capacity(function.args.len(), 4)?,
        std::mem::size_of::<Value>(),
    )?;
    let _arguments = context
        .memory
        .map(|memory| memory.scope(directory))
        .transpose()?;
    #[cfg(test)]
    super::cardinality_memory_tests::observe_argument_directory(context);
    let mut args = Vec::new();
    args.try_reserve_exact(function.args.len()).map_err(|_| {
        crate::app::CassieError::ResourceLimit("CARDINALITY argument allocation failed".into())
    })?;
    for expression in &function.args {
        check(context)?;
        args.push(argument(row, expression, context)?);
    }
    check(context)?;
    super::reject_record_arguments("cardinality", &args)?;
    #[cfg(test)]
    super::cardinality_memory_tests::observe_native_dispatch(context, &args);
    super::array_functions::cardinality(&args)
}

fn argument<R: RowAccess + ?Sized>(
    row: &R,
    expression: &Expr,
    context: EvalContext<'_>,
) -> Result<Value, QueryError> {
    let value = match expression {
        Expr::Param(index) => context.params.get(*index),
        Expr::Column(name) => super::super::super::column_value_ref(row, name, context.local_args),
        _ => {
            // Computed operands retain their ordinary operator/cast workspace.
            // Only this call's direct native leaf copies avoid the formatter.
            return super::super::super::evaluate_expr_value_with_context(row, expression, context);
        }
    };
    if let (Some(memory), Some(value)) = (context.memory, value) {
        // Vec/array directories, nested values and native coefficient payloads
        // are counted without formatting. Four copies' worth covers allocator
        // growth/limb rounding; one Value slot per node covers small payload
        // and nested inline/header rounding independently of decimal scale.
        memory.grow(native_copy_bytes(value)?)?;
    }
    check(context)?;
    #[cfg(test)]
    super::record_input_value_clone(value.is_some());
    Ok(value.cloned().unwrap_or(Value::Null))
}

fn native_copy_bytes(value: &Value) -> Result<usize, QueryError> {
    let header = std::mem::size_of::<Value>();
    let payload = mul(value_clone_bytes(value)?, 4)?;
    let nested_headers = if let Value::Array(array) = value {
        array
            .elements()
            .iter()
            .try_fold(0, |bytes, element| add(bytes, native_headers(element)?))?
    } else {
        0
    };
    Ok(add(payload, add(header, nested_headers)?)?)
}

fn native_headers(value: &Value) -> Result<usize, crate::app::CassieError> {
    let mut bytes = std::mem::size_of::<Value>();
    if let Value::Array(array) = value {
        for element in array.elements() {
            bytes = add(bytes, native_headers(element)?)?;
        }
    }
    Ok(bytes)
}

fn check(context: EvalContext<'_>) -> Result<(), QueryError> {
    if let Some(controls) = context.controls {
        controls.check_interruption()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_preserve_native_cardinality_record_rejection() {
        // Arrange
        let params = [Value::Typed(crate::types::typed::TypedValue::Record(
            crate::types::RecordValue::anonymous(vec![(
                Value::Int64(1),
                crate::types::DataType::Int,
            )]),
        ))];
        let functions = std::collections::HashMap::new();
        let row = crate::executor::batch::BatchRow::new(Vec::new());
        let call = FunctionCall {
            name: "cardinality".into(),
            args: vec![Expr::Param(0)],
            ..FunctionCall::default()
        };

        // Act
        let error = evaluate(
            &call,
            &row,
            EvalContext {
                params: &params,
                search_context: None,
                user_functions: &functions,
                local_args: None,
                session: None,
                exists: None,
                controls: None,
                memory: None,
                deletion_memory: None,
            },
        )
        .expect_err("record signature remains rejected");

        // Assert
        assert_eq!(crate::app::CassieError::from(error).sql_state(), "42883");
    }

    #[test]
    fn should_release_fallback_native_owners_after_success_and_error() {
        for deletion_only in [false, true] {
            for invalid_arity in [false, true] {
                // Arrange
                let controls = crate::runtime::QueryExecutionControls::from_limits(
                    &crate::config::CassieRuntimeLimits {
                        query_memory_budget_bytes: 32 * 1024,
                        ..Default::default()
                    },
                    std::time::Instant::now(),
                );
                let memory = EvaluationMemory::new(&controls).expect("deletion owner");
                let params = [Value::Array(
                    crate::types::SqlArray::from_elements(vec![Value::String("native".into())])
                        .expect("one element"),
                )];
                let input = controls
                    .reserve_query_memory(value_clone_bytes(&params[0]).expect("native bytes"))
                    .expect("independent input owner");
                let row = crate::executor::batch::BatchRow::new(Vec::new());
                let functions = std::collections::HashMap::new();
                let mut call = FunctionCall {
                    name: "cardinality".into(),
                    args: vec![Expr::Param(0)],
                    ..FunctionCall::default()
                };
                if invalid_arity {
                    call.args.push(Expr::Param(0));
                }

                // Act
                let result = evaluate(
                    &call,
                    &row,
                    EvalContext {
                        params: &params,
                        search_context: None,
                        user_functions: &functions,
                        local_args: None,
                        session: None,
                        exists: None,
                        controls: Some(&controls),
                        memory: None,
                        deletion_memory: deletion_only.then_some(&memory),
                    },
                );

                // Assert
                if invalid_arity {
                    assert!(result.is_err());
                } else {
                    assert_eq!(result.expect("native count"), Value::Int64(1));
                }
                assert!(controls.peak_query_memory_bytes() > input.bytes());
                assert_eq!(controls.current_query_memory_bytes(), input.bytes());
                assert_eq!(memory.bytes(), 0);
                drop((params, input, memory));
                assert_eq!(controls.current_query_memory_bytes(), 0);
            }
        }
    }
}
