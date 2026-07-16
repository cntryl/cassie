use super::{
    build_logical_plan, check_timeout, ensure_query_memory_budget_for_rows, execute_plan,
    row_signature, BatchRow, Cassie, CassieSession, CommonTableExpression, CteQuery, FunctionMeta,
    HashMap, HashSet, QueryError, QueryExecutionControls, Value,
};
use crate::sql::ast::SetOperator;

pub(super) type CteRows = Vec<Vec<(String, Value)>>;
pub(super) type CteContext = HashMap<String, CteRows>;
type CteExecution<'a> = Result<CteRows, QueryError>;

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

    let output = match &cte.query {
        CteQuery::Simple(statement) => {
            let logical = build_logical_plan(statement.as_ref())?;
            execute_plan(
                cassie,
                session,
                &logical,
                cte_context,
                user_functions,
                params,
                controls,
            )?
            .into_iter()
            .map(BatchRow::into_entries)
            .collect::<Vec<_>>()
        }
        CteQuery::Recursive {
            operator,
            base,
            recursive,
        } => {
            let base_plan = build_logical_plan(base.as_ref())?;
            let recursive_plan = build_logical_plan(recursive.as_ref())?;
            let mut rows = execute_plan(
                cassie,
                session,
                &base_plan,
                cte_context,
                user_functions,
                params,
                controls,
            )?
            .into_iter()
            .map(BatchRow::into_entries)
            .collect::<Vec<_>>();
            rows = rename_cte_rows(rows, &cte.aliases);

            let mut seen: HashSet<String> = HashSet::new();
            if matches!(operator, SetOperator::Union) {
                rows.retain(|row| seen.insert(row_signature(row)));
            }
            let mut delta = rows.clone();
            let mut memory = replace_recursive_memory(None, controls, &rows, &delta)?;
            cte_context.insert(cte_name.clone(), delta.clone());
            let mut stabilized = false;

            for _ in 0..controls.cte_recursion_depth {
                check_timeout(controls)?;
                let recursive_rows = execute_plan(
                    cassie,
                    session,
                    &recursive_plan,
                    cte_context,
                    user_functions,
                    params,
                    controls,
                )?
                .into_iter()
                .map(BatchRow::into_entries)
                .collect::<Vec<_>>();
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
                cte_context.insert(cte_name.clone(), delta.clone());
            }

            if !stabilized {
                return Err(QueryError::General(format!(
                    "recursive CTE '{}' did not stabilize within {} iterations",
                    cte.name, controls.cte_recursion_depth
                )));
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

    Ok(output)
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
                .map(|(index, (_, value))| {
                    (
                        aliases
                            .get(index)
                            .cloned()
                            .unwrap_or_else(|| format!("column_{}", index + 1)),
                        value,
                    )
                })
                .collect()
        })
        .collect()
}
