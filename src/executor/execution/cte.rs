use super::{
    build_logical_plan, check_timeout, ensure_query_memory_budget_for_rows, execute_plan,
    row_signature, BatchRow, Cassie, CassieSession, CommonTableExpression, CteQuery, FunctionMeta,
    HashMap, HashSet, QueryError, QueryExecutionControls, Value,
};
use crate::executor::semantic::SemanticKey;
use crate::sql::ast::SetOperator;

pub(super) type CteRows = Vec<Vec<(String, Value)>>;
#[derive(Clone)]
pub(super) struct CteRelation {
    pub(super) rows: CteRows,
    pub(super) fields: Vec<crate::types::FieldSchema>,
}
pub(super) type CteContext = HashMap<String, CteRelation>;
type CteExecution<'a> = Result<CteRelation, QueryError>;

pub(super) fn context_fields(
    context: &CteContext,
) -> HashMap<String, Vec<crate::types::FieldSchema>> {
    context
        .iter()
        .map(|(name, relation)| (name.clone(), relation.fields.clone()))
        .collect()
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
    let cte_name = cte.name.to_ascii_lowercase();
    let previous = cte_context.remove(&cte_name);
    let fields = crate::sql::binder::cte_row_fields(
        cte,
        &context_fields(cte_context),
        &cassie.catalog,
        user_functions,
    )
    .map_err(|error| QueryError::General(error.to_string()))?;

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
            rows = rename_cte_rows(rows, &cte.aliases);

            let mut seen: HashSet<SemanticKey> = HashSet::new();
            if matches!(operator, SetOperator::Union) {
                rows.retain(|row| seen.insert(row_signature(row)));
            }
            let mut delta = rows.clone();
            let mut memory = replace_recursive_memory(None, controls, &rows, &delta)?;
            store_working_rows(cte_context, &cte_name, &delta, &fields);
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
                let recursive_rows = rename_cte_rows(recursive_rows, &cte.aliases);

                let new_rows = match operator {
                    SetOperator::Union => recursive_rows
                        .into_iter()
                        .filter(|row| seen.insert(row_signature(row)))
                        .collect::<Vec<_>>(),
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

                rows.extend(new_rows.iter().cloned());
                delta = new_rows;
                memory = replace_recursive_memory(Some(memory), controls, &rows, &delta)?;
                store_working_rows(cte_context, &cte_name, &delta, &fields);
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

    let output = rename_cte_rows(output, &cte.aliases);
    if let Some(previous_rows) = previous {
        cte_context.insert(cte_name, previous_rows);
    } else {
        cte_context.remove(&cte_name);
    }

    Ok(CteRelation {
        rows: output,
        fields,
    })
}

fn store_working_rows(
    context: &mut CteContext,
    name: &str,
    rows: &CteRows,
    fields: &[crate::types::FieldSchema],
) {
    context.insert(
        name.to_string(),
        CteRelation {
            rows: rows.clone(),
            fields: fields.to_vec(),
        },
    );
}

fn execute_cte_plan(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    plan: &crate::planner::LogicalPlan,
    context: &mut CteContext,
    user_functions: &HashMap<String, FunctionMeta>,
    params: &[Value],
    controls: &QueryExecutionControls,
) -> Result<CteRows, QueryError> {
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
    .map(|rows| rows.into_iter().map(BatchRow::into_entries).collect())
}

fn replace_recursive_memory(
    previous: Option<(
        crate::runtime::QueryMemoryReservation,
        crate::runtime::QueryMemoryReservation,
    )>,
    controls: &QueryExecutionControls,
    rows: &CteRows,
    delta: &CteRows,
) -> Result<
    (
        crate::runtime::QueryMemoryReservation,
        crate::runtime::QueryMemoryReservation,
    ),
    QueryError,
> {
    drop(previous);
    let rows_memory = ensure_query_memory_budget_for_rows(controls, rows)?;
    let delta_memory = ensure_query_memory_budget_for_rows(controls, delta)?;
    Ok((rows_memory, delta_memory))
}

fn rename_cte_rows(rows: CteRows, aliases: &[String]) -> CteRows {
    if aliases.is_empty() || aliases.iter().any(|alias| alias == "*") {
        return rows;
    }
    rows.into_iter()
        .map(|row| {
            row.into_iter()
                .enumerate()
                .map(|(index, (name, value))| {
                    // Columns past the alias list keep their own names.
                    (aliases.get(index).cloned().unwrap_or(name), value)
                })
                .collect()
        })
        .collect()
}
