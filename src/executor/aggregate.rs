use crate::sql::ast::SelectItem;

use crate::executor::ColumnMeta;

pub fn columns_from_projection(projection: &[SelectItem]) -> Vec<ColumnMeta> {
    if projection.is_empty() {
        return vec![ColumnMeta {
            name: "*".to_string(),
            data_type: "text".to_string(),
        }];
    }

    projection
        .iter()
        .map(|item| match item {
            SelectItem::Wildcard => ColumnMeta {
                name: "*".to_string(),
                data_type: "text".to_string(),
            },
            SelectItem::Column { name, alias } => ColumnMeta {
                name: alias.clone().unwrap_or_else(|| name.clone()),
                data_type: "text".to_string(),
            },
            SelectItem::Function { function, alias } => ColumnMeta {
                name: alias.clone().unwrap_or_else(|| function.name.clone()),
                data_type: "float".to_string(),
            },
        })
        .collect()
}
