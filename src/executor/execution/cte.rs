use super::{
    build_logical_plan, check_timeout, execute_plan_with_parameter_oids, row_signature, BatchRow,
    CommonTableExpression, CteQuery, HashMap, PlanExecutionEnv, QueryError, QueryExecutionControls,
    Value,
};
#[cfg(test)]
use super::{execute_plan, Cassie};
use crate::executor::semantic::SemanticKey;
use crate::sql::ast::SetOperator;

#[cfg(test)]
mod retention_tests;
#[cfg(test)]
mod source_retention_tests;

mod context;
mod retention;
mod search_cycle;
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
    env: &'a PlanExecutionEnv<'a>,
    cte: &'a CommonTableExpression,
    cte_context: &'a mut CteContext,
) -> CteExecution<'a> {
    let controls = env.controls;
    check_timeout(controls)?;
    let _name_memory = controls.reserve_query_memory(cte.name.len())?;
    let cte_name = cte.name.to_ascii_lowercase();
    // A nonrecursive definition may read an outer CTE with the same name.
    // Only recursive evaluation installs a private working relation here.
    let recursive = matches!(cte.query, CteQuery::Recursive { .. });
    let previous = if recursive {
        cte_context.remove(&cte_name)
    } else {
        None
    };
    let namespace = context_fields(cte_context, controls)?;
    let fields = crate::sql::binder::cte_row_fields(
        cte,
        &namespace,
        &env.cassie.catalog,
        env.user_functions,
    )
    .map_err(|error| QueryError::General(error.to_string()))?;

    let fields_memory = controls.reserve_query_memory(retention::fields_bytes(&fields)?)?;
    let output = match &cte.query {
        CteQuery::Simple(statement) => {
            let logical = build_logical_plan(&env.cassie.catalog, statement.as_ref())?;
            execute_cte_plan(env, &logical, cte_context)?
        }
        CteQuery::Recursive { .. } => {
            let mut environment = RecursiveCteEnvironment { env, cte_context };
            execute_recursive_cte(&mut environment, cte, &fields)?
        }
    };

    let mut output = output;
    if cte.search.is_some() || cte.cycle.is_some() {
        search_cycle::remove_lineage(&mut output, &cte_name, cte.search.is_some(), controls)?;
    }
    let output = output.rename(&cte.aliases, controls)?;
    if let Some(previous_rows) = previous {
        cte_context.insert(&cte_name, previous_rows, controls)?;
    } else if recursive {
        cte_context.remove(&cte_name);
    }

    Ok(CteRelation {
        rows: output.rows,
        fields,
        _rows_memory: Some(output.memory),
        _fields_memory: Some(fields_memory),
    })
}

struct RecursiveCteEnvironment<'a> {
    env: &'a PlanExecutionEnv<'a>,
    cte_context: &'a mut CteContext,
}

fn execute_recursive_cte(
    environment: &mut RecursiveCteEnvironment<'_>,
    cte: &CommonTableExpression,
    fields: &[crate::types::FieldSchema],
) -> Result<RetainedRows, QueryError> {
    let CteQuery::Recursive {
        operator,
        base,
        recursive,
    } = &cte.query
    else {
        return Err(QueryError::General(
            "recursive CTE execution received a non-recursive query".into(),
        ));
    };
    let cte_name = cte.name.to_ascii_lowercase();
    let base_plan = build_logical_plan(&environment.env.cassie.catalog, base.as_ref())?;
    let recursive_statement = search_cycle::add_lineage_projection(
        recursive.as_ref(),
        &cte_name,
        cte.search.as_ref(),
        cte.cycle.as_ref(),
    )?;
    let recursive_plan = build_logical_plan(&environment.env.cassie.catalog, &recursive_statement)?;
    let mut rows = execute_cte_plan(environment.env, &base_plan, environment.cte_context)?;
    rows = rows.rename(&cte.aliases, environment.env.controls)?;
    search_cycle::append_base_metadata(
        &mut rows,
        &cte_name,
        cte.search.as_ref(),
        cte.cycle.as_ref(),
        fields,
        environment.env.controls,
    )?;

    let mut seen = seen::SeenRows::new_typed(environment.env.controls, fields)?;
    if matches!(operator, SetOperator::Union) {
        seen.retain_unique(&mut rows, environment.env.controls)?;
    }
    let mut delta = RetainedRows::copy(&rows.rows, environment.env.controls)?;
    let working_fields =
        search_cycle::working_fields(fields, &cte_name, cte.search.as_ref(), cte.cycle.as_ref());
    store_working_rows(
        environment.cte_context,
        &cte_name,
        &delta.rows,
        &working_fields,
        environment.env.controls,
    )?;
    let stabilized = execute_recursive_cte_steps(
        environment,
        cte,
        &recursive_plan,
        &working_fields,
        &mut rows,
        &mut delta,
        &mut seen,
    )?;
    if !stabilized {
        return Err(recursion_depth_exceeded(
            &cte.name,
            environment.env.controls.cte_recursion_depth,
        ));
    }
    Ok(rows)
}

fn execute_recursive_cte_steps(
    environment: &mut RecursiveCteEnvironment<'_>,
    cte: &CommonTableExpression,
    recursive_plan: &crate::planner::logical::LogicalPlan,
    working_fields: &Vec<crate::types::FieldSchema>,
    rows: &mut RetainedRows,
    delta: &mut RetainedRows,
    seen: &mut seen::SeenRows,
) -> Result<bool, QueryError> {
    let CteQuery::Recursive { operator, .. } = &cte.query else {
        return Err(QueryError::General(
            "recursive CTE execution received a non-recursive query".into(),
        ));
    };
    for _ in 0..environment.env.controls.cte_recursion_depth {
        check_timeout(environment.env.controls)?;
        let recursive_rows =
            execute_cte_plan(environment.env, recursive_plan, environment.cte_context)?;
        let mut recursive_rows = recursive_rows.rename(&cte.aliases, environment.env.controls)?;
        search_cycle::append_recursive_metadata(
            &mut recursive_rows,
            &cte.name.to_ascii_lowercase(),
            cte.search.as_ref(),
            cte.cycle.as_ref(),
            working_fields,
            environment.env.controls,
        )?;

        match operator {
            SetOperator::Union => {
                seen.retain_unique(&mut recursive_rows, environment.env.controls)?;
            }
            SetOperator::UnionAll => {}
            _ => {
                return Err(QueryError::General(
                    "unsupported recursive CTE set operator".to_string(),
                ));
            }
        }
        if recursive_rows.is_empty() {
            return Ok(true);
        }

        rows.append(&recursive_rows.rows, environment.env.controls)?;
        if cte.cycle.is_some() {
            search_cycle::prune_cycle_rows(
                &mut recursive_rows,
                &cte.name.to_ascii_lowercase(),
                environment.env.controls,
            )?;
        }
        if recursive_rows.is_empty() {
            return Ok(true);
        }

        *delta = recursive_rows;
        store_working_rows(
            environment.cte_context,
            &cte.name.to_ascii_lowercase(),
            &delta.rows,
            working_fields,
            environment.env.controls,
        )?;
    }
    Ok(false)
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
    env: &PlanExecutionEnv<'_>,
    plan: &crate::planner::LogicalPlan,
    context: &mut CteContext,
) -> Result<RetainedRows, QueryError> {
    let controls = env.controls.for_relational_scalar_cte();
    let env = PlanExecutionEnv {
        controls: &controls,
        ..*env
    };
    execute_plan_with_parameter_oids(&env, plan, context)
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
    let serializer = retention::serialization_scratch(&copied.rows)?;
    extra = add(extra, serializer)?;
    copied.memory.try_grow(extra)?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(count)
        .map_err(|error| retention::allocation(&error))?;
    for row in copied.rows {
        check_timeout(controls)?;
        output.push(BatchRow::new(row));
    }
    let retained = retention::source_backing_bytes(&output, &relation.fields, serializer)?;
    copied.memory.shrink_to(retained);
    let memory = Arc::new(copied.memory);
    for row in &mut output {
        let owned = std::mem::replace(row, BatchRow::from_projected_values(Vec::new()));
        *row = owned.with_query_memory(Some(Arc::clone(&memory)));
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
