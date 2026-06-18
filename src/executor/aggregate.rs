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
                data_type: aggregate_type(&function.name)
                    .unwrap_or("float")
                    .to_string(),
            },
        })
        .collect()
}

fn aggregate_type(name: &str) -> Option<&'static str> {
    match name.to_ascii_lowercase().as_str() {
        "count" => Some("int"),
        "sum" | "avg" => Some("float"),
        "min" | "max" => Some("text"),
        _ => None,
    }
}
