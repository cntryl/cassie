use super::{
    build_logical_plan, check_timeout, execute_plan, row_signature, BatchRow, Cassie,
    CassieSession, CommonTableExpression, CteQuery, FunctionMeta, HashMap, HashSet, QueryError,
    QueryExecutionControls, Value,
};
use crate::executor::semantic::SemanticKey;
use crate::sql::ast::SetOperator;

#[cfg(test)]
mod retention_tests;

mod context;
mod retention;
mod seen;
pub(super) use context::CteContext;
use retention::RetainedRows;

pub(super) type CteRows = Vec<Vec<(String, Value)>>;
pub(super) struct CteRelation {
    pub(super) rows: CteRows,
    pub(super) fields: Vec<crate::types::FieldSchema>,
    pub(super) _rows_memory: Option<crate::runtime::QueryMemoryReservation>,
    pub(super) _fields_memory: Option<crate::runtime::QueryMemoryReservation>,
}
impl CteRelation {
    fn copy(&self, controls: &QueryExecutionControls) -> Result<Self, QueryError> {
        let rows = RetainedRows::copy(&self.rows, controls)?;
        let fields_memory =
            controls.reserve_query_memory(retention::fields_bytes(&self.fields)?)?;
        check_timeout(controls)?;
        let fields = self.fields.clone();
        Ok(Self {
            rows: rows.rows,
            fields,
            _rows_memory: Some(rows.memory),
            _fields_memory: Some(fields_memory),
        })
    }
}
type CteExecution<'a> = Result<CteRelation, QueryError>;

pub(super) struct ContextFields {
    fields: HashMap<String, Vec<crate::types::FieldSchema>>,
    _memory: crate::runtime::QueryMemoryReservation,
}
impl std::ops::Deref for ContextFields {
    type Target = HashMap<String, Vec<crate::types::FieldSchema>>;
    fn deref(&self) -> &Self::Target {
        &self.fields
    }
}

pub(super) fn context_fields(
    context: &CteContext,
    controls: &QueryExecutionControls,
) -> Result<ContextFields, QueryError> {
    use crate::executor::retained_memory::{add, hash_table_bytes};
    check_timeout(controls)?;
    let bytes = context.iter().try_fold(
        hash_table_bytes::<(String, Vec<crate::types::FieldSchema>)>(context.len())?,
        |bytes, (name, relation)| {
            Ok::<_, QueryError>(add(
                bytes,
                add(name.len(), retention::fields_bytes(&relation.fields)?)?,
            )?)
        },
    )?;
    let memory = controls.reserve_query_memory(bytes)?;
    let mut fields = HashMap::new();
    fields
        .try_reserve(context.len())
        .map_err(|error| retention::allocation(&error))?;
    for (name, relation) in context.iter() {
        check_timeout(controls)?;
        fields.insert(name.clone(), relation.fields.clone());
    }
    Ok(ContextFields {
        fields,
        _memory: memory,
    })
}

fn recursion_depth_exceeded(name: &str, depth: usize) -> QueryError {
    QueryError::General(format!(
        "recursive CTE '{name}' exceeded the maximum recursion depth of {depth} iterations \
         (configured via CASSIE_CTE_RECURSION_DEPTH); this stops both a non-terminating \
         recursion and a legitimate one that needs more iterations than configured"
    ))
}

pub(super) fn execute_cte<'a>(
    cassie: &'a Cassie,
    session: Option<&'a CassieSession>,
    cte: &'a CommonTableExpression,
    cte_context: &'a mut CteContext,
    user_functions: &'a HashMap<String, FunctionMeta>,
    params: &'a [Value],
    controls: &'a QueryExecutionControls,
) -> CteExecution<'a> {
    check_timeout(controls)?;
    let _name_memory = controls.reserve_query_memory(cte.name.len())?;
    let cte_name = cte.name.to_ascii_lowercase();
    let previous = cte_context.remove(&cte_name);
    let namespace = context_fields(cte_context, controls)?;
    let fields =
        crate::sql::binder::cte_row_fields(cte, &namespace, &cassie.catalog, user_functions)
            .map_err(|error| QueryError::General(error.to_string()))?;

    let fields_memory = controls.reserve_query_memory(retention::fields_bytes(&fields)?)?;
    let output = match &cte.query {
        CteQuery::Simple(statement) => {
            let logical = build_logical_plan(&cassie.catalog, statement.as_ref())?;
            execute_cte_plan(
                cassie,
                session,
                &logical,
                cte_context,
                user_functions,
                params,
                controls,
            )?
        }
        CteQuery::Recursive {
            operator,
            base,
            recursive,
        } => {
            let base_plan = build_logical_plan(&cassie.catalog, base.as_ref())?;
            let recursive_plan = build_logical_plan(&cassie.catalog, recursive.as_ref())?;
            let mut rows = execute_cte_plan(
                cassie,
                session,
                &base_plan,
                cte_context,
                user_functions,
                params,
                controls,
            )?;
            rows = rows.rename(&cte.aliases, controls)?;

            let mut seen = seen::SeenRows::new(controls)?;
            if matches!(operator, SetOperator::Union) {
                seen.retain_unique(&mut rows, controls)?;
            }
            let mut delta = RetainedRows::copy(&rows.rows, controls)?;
            store_working_rows(cte_context, &cte_name, &delta.rows, &fields, controls)?;
            let mut stabilized = false;

            for _ in 0..controls.cte_recursion_depth {
                check_timeout(controls)?;
                let recursive_rows = execute_cte_plan(
                    cassie,
                    session,
                    &recursive_plan,
                    cte_context,
                    user_functions,
                    params,
                    controls,
                )?;
                let mut recursive_rows = recursive_rows.rename(&cte.aliases, controls)?;

                let new_rows = match operator {
                    SetOperator::Union => {
                        seen.retain_unique(&mut recursive_rows, controls)?;
                        recursive_rows
                    }
                    SetOperator::UnionAll => recursive_rows,
                    _ => {
                        return Err(QueryError::General(
                            "unsupported recursive CTE set operator".to_string(),
                        ));
                    }
                };

                if new_rows.is_empty() {
                    stabilized = true;
                    break;
                }

                rows.append(&new_rows.rows, controls)?;
                delta = new_rows;
                store_working_rows(cte_context, &cte_name, &delta.rows, &fields, controls)?;
            }

            if !stabilized {
                return Err(recursion_depth_exceeded(
                    &cte.name,
                    controls.cte_recursion_depth,
                ));
            }

            rows
        }
    };

    let output = output.rename(&cte.aliases, controls)?;
    if let Some(previous_rows) = previous {
        cte_context.insert(&cte_name, previous_rows, controls)?;
    } else {
        cte_context.remove(&cte_name);
    }

    Ok(CteRelation {
        rows: output.rows,
        fields,
        _rows_memory: Some(output.memory),
        _fields_memory: Some(fields_memory),
    })
}

fn store_working_rows(
    context: &mut CteContext,
    name: &str,
    rows: &CteRows,
    fields: &Vec<crate::types::FieldSchema>,
    controls: &QueryExecutionControls,
) -> Result<(), QueryError> {
    let copied = RetainedRows::copy(rows, controls)?;
    let fields_memory = controls.reserve_query_memory(retention::fields_bytes(fields)?)?;
    let relation = CteRelation {
        rows: copied.rows,
        fields: fields.clone(),
        _rows_memory: Some(copied.memory),
        _fields_memory: Some(fields_memory),
    };
    context.insert(name, relation, controls)?;
    Ok(())
}

fn execute_cte_plan(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: &crate::planner::LogicalPlan,
    context: &mut CteContext,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<RetainedRows, QueryError> {
    let controls = controls.for_relational_scalar_cte();
    execute_plan(
        cassie,
        session,
        plan,
        context,
        user_functions,
        params,
        &controls,
    )
    .and_then(|rows| RetainedRows::from_output(rows, &controls))
}

pub(super) fn copy_source_rows(
    relation: &CteRelation,
    controls: &QueryExecutionControls,
    qualifier: Option<&str>,
) -> Result<Vec<BatchRow>, QueryError> {
    use crate::executor::retained_memory::{add, lookup_bytes, mul};
    use std::mem::size_of;
    use std::sync::Arc;
    let count = relation.rows.len();
    if count == 0 {
        check_timeout(controls)?;
        return Ok(Vec::new());
    }
    let mut copied = RetainedRows::copy(&relation.rows, controls)?;
    let qualifier_bytes = qualifier.map_or(0, str::len);
    let mut extra = add(
        mul(count, 2 * size_of::<BatchRow>())?,
        add(
            retention::fields_bytes(&relation.fields)?,
            size_of::<crate::runtime::QueryMemoryReservation>() + 2 * size_of::<usize>(),
        )?,
    )?;
    extra = add(
        extra,
        mul(
            count.div_ceil(crate::executor::batch::DEFAULT_BATCH_SIZE),
            2 * size_of::<super::Batch>(),
        )?,
    )?;
    for row in &copied.rows {
        let names = row
            .iter()
            .try_fold(0, |bytes, (name, _)| add(bytes, name.len()))?;
        let names = add(names, mul(row.len(), qualifier_bytes)?)?;
        // Lookup normalization, text-field inference and qualification alias backing overlap.
        extra = add(
            extra,
            add(
                512,
                add(
                    mul(names, 64)?,
                    mul(row.len(), 16 * size_of::<(String, usize)>())?,
                )?,
            )?,
        )?;
        extra = add(extra, lookup_bytes(mul(row.len(), 8)?, mul(names, 8)?)?)?;
    }
    extra = add(extra, retention::serialization_scratch(&copied.rows)?)?;
    copied.memory.try_grow(extra)?;
    let memory = Arc::new(copied.memory);
    let mut output = Vec::new();
    output
        .try_reserve_exact(count)
        .map_err(|error| retention::allocation(&error))?;
    for row in copied.rows {
        check_timeout(controls)?;
        output.push(BatchRow::new(row).with_query_memory(Some(Arc::clone(&memory))));
    }
    check_timeout(controls)?;
    Ok(output)
}

#[cfg(test)]
mod supplemental_tests;

pub(super) fn source_fields_bytes(
    fields: &Vec<crate::types::FieldSchema>,
) -> Result<usize, QueryError> {
    retention::fields_bytes(fields)
}
