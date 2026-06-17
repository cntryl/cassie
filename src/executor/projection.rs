use crate::executor::filter::SearchContext;
use crate::executor::QueryError;
use crate::executor::filter;
use crate::sql::ast::SelectItem;
use crate::types::Value;

pub(crate) fn project_rows(
    rows: Vec<Vec<(String, Value)>>,
    projection: &[SelectItem],
    params: &[Value],
    search_context: Option<&SearchContext>,
) -> Result<Vec<Vec<(String, Value)>>, QueryError> {
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let mut projected = Vec::with_capacity(projection.len());
        for item in projection {
            match item {
                SelectItem::Wildcard => {
                    projected.extend(row.iter().map(|(name, value)| (name.clone(), value.clone())));
                }
                SelectItem::Column { name, alias } => {
                    let key = alias.as_deref().unwrap_or(name);
                    let value = row
                        .iter()
                        .find(|(column, _)| column == name)
                        .map(|(_, value)| value.clone())
                        .unwrap_or(Value::Null);
                    projected.push((key.to_string(), value));
                }
                SelectItem::Function { function, alias } => {
                    let key = alias
                        .as_deref()
                        .unwrap_or(function.name.as_str())
                        .to_string();
                    let value = filter::evaluate_expr_value(
                        &row,
                        &crate::sql::ast::Expr::Function(function.clone()),
                        params,
                        search_context,
                    )?;
                    projected.push((key, value));
                }
            }
        }
        out.push(projected);
    }

    Ok(out)
}
