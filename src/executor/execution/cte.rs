use super::*;

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
            .collect()
        }
        CteQuery::Recursive { base, recursive } => {
            let base_plan = build_logical_plan(base.as_ref())?;
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

            cte_context.insert(cte_name.clone(), rows.clone());

            let mut seen: HashSet<String> = rows.iter().map(row_signature).collect();
            let mut stabilized = false;

            for _ in 0..controls.cte_recursion_depth {
                check_timeout(controls)?;
                let recursive_plan = build_logical_plan(recursive.as_ref())?;
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

                let mut new_rows = Vec::new();
                for row in recursive_rows {
                    let signature = row_signature(&row);
                    if seen.insert(signature) {
                        rows.push(row.clone());
                        new_rows.push(row);
                    }
                }

                if new_rows.is_empty() {
                    stabilized = true;
                    break;
                }

                ensure_temp_budget_for_rows(controls, &rows)?;
                cte_context.insert(cte_name.clone(), rows.clone());
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

    if let Some(previous_rows) = previous {
        cte_context.insert(cte_name, previous_rows);
    } else {
        cte_context.remove(&cte_name);
    }

    Ok(output)
}
