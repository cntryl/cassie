//! Private ownership of pattern arguments across nested translation and matching.

use super::pattern_memory::{reserve, PatternText};
use super::{check_interruption, resource_limit, QueryError};
use crate::executor::filter::EvalContext;
use crate::{
    executor::{batch::RowAccess, retained_memory::value_clone_bytes},
    runtime::{QueryExecutionControls, QueryMemoryReservation},
    sql::ast::{Expr, FunctionCall},
    types::Value,
};
use std::mem::size_of;

pub(super) struct Arguments {
    pub(super) values: Vec<Value>,
    leases: Vec<Option<QueryMemoryReservation>>,
    _directory: Option<QueryMemoryReservation>,
}

struct Argument {
    value: Value,
    lease: Option<QueryMemoryReservation>,
}

impl Arguments {
    fn new(count: usize, controls: Option<&QueryExecutionControls>) -> Result<Self, QueryError> {
        let bytes = count
            .checked_mul(size_of::<Value>() + size_of::<Option<QueryMemoryReservation>>())
            .ok_or_else(|| resource_limit("pattern argument directory size overflow"))?;
        let directory = reserve(controls, bytes)?;
        let mut values = Vec::new();
        let mut leases = Vec::new();
        values.try_reserve_exact(count).map_err(|error| {
            resource_limit(&format!("unable to allocate pattern arguments: {error}"))
        })?;
        leases.try_reserve_exact(count).map_err(|error| {
            resource_limit(&format!(
                "unable to allocate pattern argument leases: {error}"
            ))
        })?;
        if values.capacity() > count || leases.capacity() > count {
            return Err(resource_limit(
                "pattern argument directory exceeded admission",
            ));
        }
        Ok(Self {
            values,
            leases,
            _directory: directory,
        })
    }

    fn push(&mut self, argument: Argument) {
        self.values.push(argument.value);
        self.leases.push(argument.lease);
    }
}

pub(super) fn collect<R: RowAccess + ?Sized>(
    function: &FunctionCall,
    row: &R,
    context: EvalContext<'_>,
    controls: Option<&QueryExecutionControls>,
) -> Result<Arguments, QueryError> {
    check_interruption(controls)?;
    let mut arguments = Arguments::new(function.args.len(), controls)?;
    for (index, expression) in function.args.iter().enumerate() {
        check_interruption(controls)?;
        let argument = evaluate(expression, function, index, row, context, controls)?;
        arguments.push(argument);
    }
    Ok(arguments)
}

fn evaluate<R: RowAccess + ?Sized>(
    expression: &Expr,
    function: &FunctionCall,
    index: usize,
    row: &R,
    context: EvalContext<'_>,
    controls: Option<&QueryExecutionControls>,
) -> Result<Argument, QueryError> {
    match expression {
        Expr::StringLiteral(text) => Ok(owned_text(PatternText::copy(text, controls)?)),
        Expr::Param(index) => {
            clone_value(context.params.get(*index).unwrap_or(&Value::Null), controls)
        }
        Expr::Column(name) => clone_value(
            crate::executor::filter::column_value_ref(row, name, context.local_args)
                .unwrap_or(&Value::Null),
            controls,
        ),
        Expr::Function(helper)
            if crate::sql::functions::function(&helper.name).is_some_and(|metadata| {
                metadata.id == crate::sql::functions::FunctionId::SimilarToEscape
            }) =>
        {
            let arguments = collect(helper, row, context, controls)?;
            match super::similar_pattern::evaluate_owned(&arguments.values, controls)? {
                Some(text) => Ok(owned_text(text)),
                None => Ok(Argument {
                    value: Value::Null,
                    lease: None,
                }),
            }
        }
        _ => {
            // Other scalar producers retain their existing pre-allocation
            // contract. Adopt their returned backing before native work and
            // retain it through the enclosing pattern call (#849 qualification).
            let value = crate::executor::filter::functions::text_selection::literal_argument(
                &function.name,
                index,
                expression,
                &function.args,
                context.params,
            )
            .or_else(|| {
                crate::executor::filter::conditional::evaluate_argument(
                    &function.name,
                    expression,
                    row,
                    context,
                )
            })
            .unwrap_or_else(|| {
                crate::executor::filter::evaluate_expr_value_with_context(row, expression, context)
            })?;
            let lease = reserve(controls, value_clone_bytes(&value)?)?;
            Ok(Argument { value, lease })
        }
    }
}

fn owned_text(text: PatternText) -> Argument {
    let (text, lease) = text.into_parts();
    Argument {
        value: Value::String(text),
        lease,
    }
}

fn clone_value(
    value: &Value,
    controls: Option<&QueryExecutionControls>,
) -> Result<Argument, QueryError> {
    if let Value::String(text) = value {
        return Ok(owned_text(PatternText::copy(text, controls)?));
    }
    let lease = reserve(controls, value_clone_bytes(value)?)?;
    Ok(Argument {
        value: value.clone(),
        lease,
    })
}

#[cfg(test)]
mod tests {
    use super::collect;
    use crate::{
        config::CassieRuntimeLimits,
        executor::{batch::BatchRow, filter::EvalContext},
        runtime::QueryExecutionControls,
        sql::ast::{Expr, FunctionCall},
        types::Value,
    };

    #[test]
    fn should_skip_pattern_directories_for_cancelled_zero_budget_requests() {
        // Arrange
        let limits = CassieRuntimeLimits {
            query_memory_budget_bytes: 0,
            ..CassieRuntimeLimits::default()
        };
        let cancellation = crate::runtime::QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &limits,
            std::time::Instant::now(),
            cancellation.clone(),
        );
        cancellation.cancel();
        let functions = std::collections::HashMap::new();
        let row = BatchRow::new(Vec::new());
        let function = FunctionCall {
            name: "regexp_like".into(),
            args: vec![
                Expr::StringLiteral("x".into()),
                Expr::StringLiteral("x".into()),
            ],
            ..Default::default()
        };
        let context = EvalContext {
            params: &[],
            search_context: None,
            user_functions: &functions,
            local_args: None,
            session: None,
            exists: None,
            controls: Some(&controls),
        };

        // Act
        let result = collect(&function, &row, context, Some(&controls));

        // Assert
        assert!(matches!(
            result,
            Err(crate::executor::filter::QueryError::Cassie(
                crate::app::CassieError::QueryCancelled
            ))
        ));
        assert_eq!(controls.peak_query_memory_bytes(), 0);
    }

    #[test]
    fn should_keep_nested_similar_output_admitted_through_parent_arguments() {
        // Arrange
        let controls = QueryExecutionControls::from_limits(
            &CassieRuntimeLimits::default(),
            std::time::Instant::now(),
        );
        let functions = std::collections::HashMap::new();
        let row = BatchRow::new(Vec::new());
        let helper = FunctionCall {
            name: "similar_to_escape".into(),
            args: vec![Expr::StringLiteral("a%b".into())],
            ..Default::default()
        };
        let outer = FunctionCall {
            name: "regexp_like".into(),
            args: vec![
                Expr::StringLiteral("abc".into()),
                Expr::Function(Box::new(helper)),
            ],
            ..Default::default()
        };
        let context = EvalContext {
            params: &[],
            search_context: None,
            user_functions: &functions,
            local_args: None,
            session: None,
            exists: None,
            controls: Some(&controls),
        };

        // Act
        let arguments =
            collect(&outer, &row, context, Some(&controls)).expect("nested pattern arguments");
        let retained = controls.current_query_memory_bytes();
        let matched = super::super::regexp_like(&arguments.values, Some(&controls))
            .expect("native match with retained argument owners");
        let after_match = controls.current_query_memory_bytes();
        drop(arguments);

        // Assert
        assert_eq!(matched, Value::Bool(true));
        assert!(retained >= 13);
        assert_eq!(after_match, retained);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
