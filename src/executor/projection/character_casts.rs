//! Retain data-dependent character-array cast scratch through projected rows.
use super::super::filter::evaluation::{
    evaluate_projection_value_with_owners, ProjectionValueContext,
};
use super::super::filter::EvaluationMemory;
use super::{BatchRow, CassieSession, ProjectionOp, QueryError, RowAccess};
use crate::catalog::FunctionMeta;
use crate::runtime::QueryExecutionControls;
use crate::types::{DataType, Value};
use std::collections::HashMap;
use std::sync::Arc;

#[cfg(test)]
thread_local! {
    static CANCEL_BEFORE_ATTACH: std::cell::RefCell<Option<crate::runtime::QueryCancellationHandle>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn cancel_before_attach_once(handle: crate::runtime::QueryCancellationHandle) {
    CANCEL_BEFORE_ATTACH.with(|slot| *slot.borrow_mut() = Some(handle));
}

pub(super) fn required(ops: &[ProjectionOp]) -> bool {
    ops.iter().any(|op| match op {
        ProjectionOp::ScalarFunction { expr, .. } => expr.any_descendant_or_self(&mut |expr| {
            matches!(expr, crate::sql::ast::Expr::Cast { data_type: DataType::Array(element), .. }
                if matches!(element.as_ref(), DataType::Text | DataType::Char { .. } | DataType::Varchar { .. }))
        }),
        _ => false,
    })
}

pub(super) fn deletion_required<R: RowAccess>(
    ops: &[ProjectionOp],
    row: &R,
    params: &[Value],
    functions: &HashMap<String, FunctionMeta>,
) -> bool {
    ops.iter().any(|op| match op {
        ProjectionOp::ScalarFunction { expr, .. } => {
            expr.any_descendant_or_self(&mut |expr| match expr {
                crate::sql::ast::Expr::Binary {
                    op:
                        crate::sql::ast::BinaryOp::JsonbDeletePath
                        | crate::sql::ast::BinaryOp::JsonExtract { .. },
                    ..
                } => true,
                crate::sql::ast::Expr::Binary {
                    left,
                    op: crate::sql::ast::BinaryOp::Sub,
                    ..
                } => json_producer(left, row, params, functions),
                crate::sql::ast::Expr::Binary {
                    left,
                    op: crate::sql::ast::BinaryOp::Concat,
                    right,
                } => {
                    json_producer(left, row, params, functions)
                        || json_producer(right, row, params, functions)
                }
                _ => false,
            })
        }
        _ => false,
    })
}

fn json_producer<R: RowAccess>(
    expression: &crate::sql::ast::Expr,
    row: &R,
    params: &[Value],
    functions: &HashMap<String, FunctionMeta>,
) -> bool {
    use crate::sql::ast::{BinaryOp, Expr};
    match expression {
        Expr::Column(name) => {
            row.column_type(name).is_some_and(DataType::is_json)
                || matches!(row.get(name), Some(Value::Json(_)))
        }
        Expr::Param(index) => matches!(params.get(*index), Some(Value::Json(_))),
        Expr::Cast { data_type, .. } => data_type.is_json(),
        Expr::Binary {
            left,
            op: BinaryOp::Sub | BinaryOp::JsonbDeletePath,
            ..
        } => json_producer(left, row, params, functions),
        Expr::Binary {
            left,
            op: BinaryOp::Concat,
            right,
        } => {
            json_producer(left, row, params, functions)
                || json_producer(right, row, params, functions)
        }
        Expr::Function(function) => {
            functions
                .get(&function.name)
                .is_some_and(|metadata| metadata.return_type.is_json())
                || crate::sql::functions::function(&function.name).is_some_and(|metadata| {
                    matches!(
                        metadata.return_type,
                        crate::sql::functions::FunctionReturnType::Json
                            | crate::sql::functions::FunctionReturnType::Jsonb
                    )
                })
        }
        _ => false,
    }
}

pub(super) struct Memory<'a> {
    controls: Option<&'a QueryExecutionControls>,
    evaluation: Option<EvaluationMemory>,
    deletion: Option<EvaluationMemory>,
}

struct Pending {
    row: BatchRow,
    memory: Option<EvaluationMemory>,
    deletion: Option<EvaluationMemory>,
    shared: Option<Arc<crate::runtime::QueryMemoryReservation>>,
}
impl<'a> Memory<'a> {
    pub(super) fn new(
        required: bool,
        deletion_required: bool,
        session: Option<&'a CassieSession>,
    ) -> Result<Self, QueryError> {
        let controls = session
            .filter(|_| required || deletion_required)
            .and_then(CassieSession::statement_execution_controls);
        Ok(Self {
            controls,
            evaluation: controls
                .filter(|_| required)
                .map(EvaluationMemory::new)
                .transpose()?,
            deletion: controls
                .filter(|_| deletion_required)
                .map(EvaluationMemory::new)
                .transpose()?,
        })
    }
    pub(super) fn evaluate<R: RowAccess + ?Sized>(
        &self,
        row: &R,
        expr: &crate::sql::ast::Expr,
        context: &ProjectionValueContext<'_>,
    ) -> Option<Result<crate::types::Value, QueryError>> {
        if self.evaluation.is_none() && self.deletion.is_none() {
            return None;
        }
        Some(evaluate_projection_value_with_owners(
            row,
            expr,
            context,
            self.controls?,
            self.evaluation.as_ref(),
            self.deletion.as_ref(),
        ))
    }
    pub(super) fn retain(self, row: BatchRow) -> Result<BatchRow, QueryError> {
        let Some(controls) = self.controls else {
            return Ok(row);
        };
        let mut pending = Pending {
            row,
            memory: self.evaluation,
            deletion: self.deletion,
            shared: None,
        };
        if pending
            .deletion
            .as_ref()
            .is_some_and(|owner| owner.bytes() == 0)
        {
            drop(pending.deletion.take());
        }
        if let (Some(memory), Some(deletion)) = (&pending.memory, &pending.deletion) {
            if !memory.belongs_to(controls) || !deletion.belongs_to(controls) {
                return Err(crate::app::CassieError::ResourceLimit(
                    "projection owners use different query trackers".into(),
                )
                .into());
            }
            let deletion = pending.deletion.take().expect("deletion owner");
            memory.adopt(deletion.into_reservation());
        }
        if pending.memory.is_none() {
            pending.memory = pending.deletion.take();
        }
        let Some(memory) = pending.memory.as_ref() else {
            return Ok(pending.row);
        };
        memory.grow(
            std::mem::size_of::<crate::runtime::QueryMemoryReservation>()
                + 2 * std::mem::size_of::<usize>(),
        )?;
        let memory = pending
            .memory
            .take()
            .expect("projection owner")
            .into_reservation();
        pending.shared = Some(Arc::new(memory));
        #[cfg(test)]
        CANCEL_BEFORE_ATTACH.with(|slot| {
            if let Some(handle) = slot.borrow_mut().take() {
                handle.cancel();
            }
        });
        controls.check_interruption()?;
        pending.row.attach_operator_memory(
            controls,
            Arc::clone(pending.shared.as_ref().expect("owner")),
        )?;
        Ok(pending.row)
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
            evaluation: Some(EvaluationMemory::new(&controls).expect("projection owner")),
            deletion: None,
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

    #[test]
    fn should_release_the_projected_row_before_the_owner_on_attachment_failure() {
        // Arrange
        let limits = crate::config::CassieRuntimeLimits {
            query_memory_budget_bytes: 1024,
            query_timeout_ms: 0,
            ..crate::config::CassieRuntimeLimits::default()
        };
        let controls = QueryExecutionControls::from_limits(&limits, Instant::now());
        let deletion = EvaluationMemory::new(&controls).expect("owner");
        deletion.grow(1024).expect("output reservation");
        let memory = Memory {
            controls: Some(&controls),
            evaluation: None,
            deletion: Some(deletion),
        };
        let row = BatchRow::new(vec![("value".into(), Value::String("x".repeat(1024)))]);
        // Act
        let result = memory.retain(row);
        // Assert
        assert!(result.is_err());
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_keep_existing_query_owner_when_attaching_the_deletion_owner() {
        // Arrange
        let controls = QueryExecutionControls::from_limits(
            &crate::config::CassieRuntimeLimits::default(),
            Instant::now(),
        );
        let source = Arc::new(controls.reserve_query_memory(64).expect("source"));
        let weak = Arc::downgrade(&source);
        let deletion = EvaluationMemory::new(&controls).expect("owner");
        deletion.grow(128).expect("output reservation");
        let memory = Memory {
            controls: Some(&controls),
            evaluation: None,
            deletion: Some(deletion),
        };
        let row = BatchRow::new(vec![("value".into(), Value::String("x".repeat(128)))])
            .with_query_memory(Some(source));
        // Act
        let row = memory.retain(row).expect("attached");
        // Assert
        assert!(weak.upgrade().is_some());
        drop(row);
        assert!(weak.upgrade().is_none());
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
