//! Pull one admitted UNNEST row at a time from retained native array arguments.

use std::mem::size_of;
use std::sync::Arc;

use super::{BatchRow, Expr, QueryError, SourceExecution, SourceExecutionEnv, Value};
use crate::executor::retained_memory::{
    add, data_type_clone_bytes, lookup_bytes, mul, value_clone_bytes,
};
use crate::runtime::QueryMemoryReservation;
use crate::sql::ast::FunctionCall;

#[cfg(test)]
#[path = "source_set_probe.rs"]
pub(super) mod probe;

pub(super) struct Cursor {
    arguments: Vec<Value>,
    position: usize,
    length: usize,
    ordinal: bool,
    types: Vec<crate::types::DataType>,
    _memory: QueryMemoryReservation,
}

impl Cursor {
    pub(super) fn open(
        env: &SourceExecutionEnv<'_>,
        function: &FunctionCall,
        ordinal: bool,
        outer: Option<&BatchRow>,
        ctes: &super::CteContext,
    ) -> Result<Self, QueryError> {
        env.controls.check_interruption()?;
        let empty = BatchRow::from_projected_values(Vec::new());
        let row = outer.unwrap_or(&empty);
        let mut memory = env
            .controls
            .reserve_query_memory(mul(function.args.len(), size_of::<Value>())?)?;
        let mut arguments = Vec::with_capacity(function.args.len());
        let empty_schema = crate::types::Schema { fields: Vec::new() };
        memory.try_grow(mul(
            function.args.len() + usize::from(ordinal),
            size_of::<crate::types::DataType>()
                + size_of::<(String, crate::types::DataType)>()
                + 26,
        )?)?;
        let columns = crate::sql::table_sources::unnest_columns(function, ordinal, |argument| {
            crate::sql::binder::infer_expr_type(argument, &empty_schema, env.user_functions, &[])
        })?;
        let types = columns
            .into_iter()
            .map(|(_, data_type)| data_type)
            .collect();
        let mut length = 0;
        for expression in &function.args {
            env.controls.check_interruption()?;
            let _scratch = env
                .controls
                .reserve_query_memory(argument_scratch(expression, env.params, row)?)?;
            let evaluation_memory = super::filter::EvaluationMemory::new(env.controls)?;
            let value = argument_value(env, expression, row, ctes, &evaluation_memory)?;
            memory.try_grow(value_clone_bytes(&value)?)?;
            drop(evaluation_memory);
            match &value {
                Value::Array(array) => length = length.max(array.elements().len()),
                Value::Null => {}
                _ => {
                    return Err(crate::app::CassieError::Execution(
                        "function unnest(non-array) does not exist".into(),
                    )
                    .into())
                }
            }
            arguments.push(value);
        }
        Ok(Self {
            arguments,
            position: 0,
            length,
            ordinal,
            types,
            _memory: memory,
        })
    }

    pub(super) fn next(
        &mut self,
        env: &SourceExecutionEnv<'_>,
    ) -> Result<Option<BatchRow>, QueryError> {
        env.controls.check_interruption()?;
        if self.position == self.length {
            return Ok(None);
        }
        let count = self.arguments.len() + usize::from(self.ordinal);
        let names = mul(count, 26)?;
        let mut bytes = add(
            mul(count, size_of::<(String, Value)>())?,
            add(names, lookup_bytes(count, names)?)?,
        )?;
        bytes = add(
            bytes,
            size_of::<BatchRow>() + size_of::<QueryMemoryReservation>() + 9 * size_of::<usize>(),
        )?;
        bytes = add(bytes, mul(count, size_of::<crate::types::DataType>())?)?;
        for kind in &self.types {
            bytes = add(bytes, data_type_clone_bytes(kind)?)?;
        }
        for argument in &self.arguments {
            if let Value::Array(array) = argument {
                if let Some(value) = array.elements().get(self.position) {
                    bytes = add(bytes, value_clone_bytes(value)?)?;
                }
            }
        }
        let memory = Arc::new(env.controls.reserve_query_memory(bytes)?);
        let mut values = Vec::with_capacity(count);
        for (index, argument) in self.arguments.iter().enumerate() {
            let value = match argument {
                Value::Array(array) => array
                    .elements()
                    .get(self.position)
                    .cloned()
                    .unwrap_or(Value::Null),
                _ => Value::Null,
            };
            values.push((crate::sql::table_sources::physical_name(index), value));
        }
        self.position += 1;
        if self.ordinal {
            let ordinal = i64::try_from(self.position).map_err(|_| {
                crate::app::CassieError::ResourceLimit("UNNEST ordinality overflow".into())
            })?;
            values.push((
                crate::sql::table_sources::physical_name(self.arguments.len()),
                Value::Int64(ordinal),
            ));
        }
        #[cfg(test)]
        probe::after_row();
        Ok(Some(
            BatchRow::from_projected_values(values)
                .with_optional_data_types(Some(Arc::new(self.types.clone())))
                .with_query_memory(Some(memory)),
        ))
    }
}

pub(super) fn execute(
    env: &SourceExecutionEnv<'_>,
    function: &FunctionCall,
    ordinal: bool,
    outer: Option<&BatchRow>,
    row_budget: Option<usize>,
    ctes: &super::CteContext,
) -> SourceExecution {
    if row_budget == Some(0) {
        return Ok((Vec::new(), Vec::new()));
    }
    let mut cursor = Cursor::open(env, function, ordinal, outer, ctes)?;
    let mut rows = Vec::new();
    let mut memory = env.controls.reserve_query_memory(0)?;
    while rows.len() < row_budget.unwrap_or(usize::MAX) {
        let Some(row) = cursor.next(env)? else {
            break;
        };
        memory.try_grow(size_of::<BatchRow>())?;
        rows.try_reserve_exact(1).map_err(|error| {
            crate::app::CassieError::ResourceLimit(format!(
                "unable to retain UNNEST input: {error}"
            ))
        })?;
        rows.push(row);
    }
    let memory = Arc::new(memory);
    for row in &mut rows {
        row.attach_operator_memory(env.controls, Arc::clone(&memory))?;
    }
    Ok((
        super::batch::chunk_rows(rows, super::batch::DEFAULT_BATCH_SIZE),
        Vec::new(),
    ))
}

pub(in crate::executor::execution) fn argument_scratch(
    expression: &Expr,
    params: &[Value],
    row: &BatchRow,
) -> Result<usize, QueryError> {
    let own = match expression {
        Expr::Column(name) => value_clone_bytes(row.get(name).unwrap_or(&Value::Null))?,
        Expr::Param(index) => params.get(*index).map_or(Ok(0), value_clone_bytes)?,
        Expr::StringLiteral(text) => text.len(),
        _ => 0,
    };
    let mut bytes = add(mul(own, 32)?, 1024)?;
    expression.try_visit_children(|child| {
        bytes = add(bytes, argument_scratch(child, params, row)?)?;
        Ok::<_, QueryError>(())
    })?;
    Ok(bytes)
}

pub(super) fn argument_value(
    env: &SourceExecutionEnv<'_>,
    expression: &Expr,
    row: &BatchRow,
    ctes: &super::CteContext,
    memory: &super::filter::EvaluationMemory,
) -> Result<Value, QueryError> {
    if !expression.contains_subquery() {
        return super::filter::evaluate_source_argument(
            row,
            expression,
            env.params,
            env.user_functions,
            env.session,
            env.controls,
            memory,
        );
    }
    let context = super::ExistsResolutionContext {
        cassie: env.cassie,
        session: env.session,
        cte_context: ctes,
        user_functions: env.user_functions,
        params: env.params,
        parameter_type_oids: env.parameter_type_oids,
        controls: env.controls,
        outer_row: Some(row),
    };
    let resolver = |expr: &Expr| super::resolve_exists_expr(&context, expr);
    super::filter::evaluate_source_argument_resolving(
        row,
        expression,
        super::filter::ExistsValueContext {
            params: env.params,
            search: None,
            functions: env.user_functions,
            session: env.session,
            resolver: &resolver,
        },
        env.controls,
        memory,
    )
}
