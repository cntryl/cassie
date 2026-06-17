use crate::executor::QueryError;
use crate::executor::filter;
use crate::executor::filter::SearchContext;
use crate::sql::ast::SelectItem;
use crate::types::Value;

pub(crate) fn project(
    rows: Vec<Vec<(String, Value)>>,
    projection: &[SelectItem],
    params: &[Value],
    search_context: Option<&SearchContext>,
) -> Result<Vec<Vec<Value>>, QueryError> {
    let mut out = Vec::with_capacity(rows.len());
    if projection.is_empty() {
        for row in rows {
            out.push(row.into_iter().map(|(_, v)| v).collect());
        }
        return Ok(out);
    }

    for row in rows {
        let mut projected = Vec::with_capacity(projection.len());
        for item in projection {
            match item {
                SelectItem::Wildcard => {
                    projected.extend(row.iter().map(|(_, v)| v.clone()));
                }
                SelectItem::Column { name, .. } => {
                    if let Some((_, v)) = row.iter().find(|(n, _)| n == name) {
                        projected.push(v.clone());
                    } else {
                        projected.push(Value::Null);
                    }
                }
                SelectItem::Function { function, .. } => {
                    let value = filter::evaluate_expr_value(
                        &row,
                        &crate::sql::ast::Expr::Function(function.clone()),
                        params,
                        search_context,
                    )?;
                    projected.push(value);
                }
            }
        }
        out.push(projected);
    }

    Ok(out)
}
