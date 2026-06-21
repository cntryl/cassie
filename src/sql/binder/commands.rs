use super::*;

pub(super) fn bind_insert(
    mut statement: crate::sql::ast::InsertStatement,
    catalog: &Catalog,
) -> Result<crate::sql::ast::InsertStatement, CassieError> {
    let table = statement.table.trim().to_string();
    if table.is_empty() {
        return Err(CassieError::Planner(
            "INSERT requires a target table".into(),
        ));
    }
    if virtual_views::schema(&table).is_some() || catalog.get_view(&table).is_some() {
        return Err(CassieError::Unsupported(format!(
            "relation '{table}' is read-only"
        )));
    }
    if !catalog.exists(&table) {
        return Err(CassieError::CollectionNotFound(table));
    }

    let schema = catalog
        .get_schema(&table)
        .ok_or_else(|| CassieError::CollectionNotFound(table.clone()))?;

    let mut seen_columns = HashSet::new();
    for column in statement.columns.iter_mut() {
        let column_name = column.trim().to_string();
        if column_name.is_empty() {
            return Err(CassieError::Planner(
                "INSERT column names cannot be empty".into(),
            ));
        }

        if !schema
            .fields
            .iter()
            .any(|field| field.name.eq_ignore_ascii_case(&column_name))
        {
            return Err(CassieError::Planner(format!(
                "INSERT target column '{column_name}' does not exist in '{table}'"
            )));
        }

        if !seen_columns.insert(column_name.clone()) {
            return Err(CassieError::Planner(format!(
                "INSERT column '{column_name}' is duplicated"
            )));
        }

        *column = column_name;
    }

    if let InsertSource::Select(select) = statement.source {
        let source = bind_select(*select, catalog, &HashMap::new())?;
        statement.source = InsertSource::Select(Box::new(source));
    }

    validate_returning_items(&statement.returning, &schema, &table, "INSERT", catalog)?;

    statement.table = table;
    Ok(statement)
}

pub(super) fn bind_update(
    mut statement: crate::sql::ast::UpdateStatement,
    catalog: &Catalog,
) -> Result<crate::sql::ast::UpdateStatement, CassieError> {
    let table = statement.table.trim().to_string();
    if table.is_empty() {
        return Err(CassieError::Planner(
            "UPDATE requires a target table".into(),
        ));
    }
    if virtual_views::schema(&table).is_some() || catalog.get_view(&table).is_some() {
        return Err(CassieError::Unsupported(format!(
            "relation '{table}' is read-only"
        )));
    }
    if !catalog.exists(&table) {
        return Err(CassieError::CollectionNotFound(table));
    }

    let schema = catalog
        .get_schema(&table)
        .ok_or_else(|| CassieError::CollectionNotFound(table.clone()))?;

    let mut seen = HashSet::new();
    for (field, _) in &mut statement.assignments {
        let normalized_field = field.trim().to_string();
        if normalized_field.is_empty() {
            return Err(CassieError::Planner(
                "UPDATE assignment names cannot be empty".into(),
            ));
        }
        if !schema
            .fields
            .iter()
            .any(|entry| entry.name.eq_ignore_ascii_case(&normalized_field))
        {
            return Err(CassieError::Planner(format!(
                "UPDATE assignment target '{normalized_field}' does not exist in '{table}'"
            )));
        }

        if !seen.insert(normalized_field.clone()) {
            return Err(CassieError::Planner(format!(
                "UPDATE assignment target '{normalized_field}' is duplicated"
            )));
        }

        *field = normalized_field;
    }

    validate_returning_items(&statement.returning, &schema, &table, "UPDATE", catalog)?;

    statement.table = table;
    Ok(statement)
}

pub(super) fn bind_delete(
    mut statement: crate::sql::ast::DeleteStatement,
    catalog: &Catalog,
) -> Result<crate::sql::ast::DeleteStatement, CassieError> {
    let table = statement.table.trim().to_string();
    if table.is_empty() {
        return Err(CassieError::Planner(
            "DELETE requires a target table".into(),
        ));
    }
    if virtual_views::schema(&table).is_some() || catalog.get_view(&table).is_some() {
        return Err(CassieError::Unsupported(format!(
            "relation '{table}' is read-only"
        )));
    }
    if !catalog.exists(&table) {
        return Err(CassieError::CollectionNotFound(table));
    }
    let schema = catalog
        .get_schema(&table)
        .ok_or_else(|| CassieError::CollectionNotFound(table.clone()))?;

    validate_returning_items(&statement.returning, &schema, &table, "DELETE", catalog)?;

    statement.table = table;
    Ok(statement)
}

pub(super) fn validate_returning_items(
    returning: &[SelectItem],
    schema: &CollectionSchema,
    table: &str,
    operation: &str,
    catalog: &Catalog,
) -> Result<(), CassieError> {
    let mut known_fields = schema
        .fields
        .iter()
        .map(|field| field.name.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    known_fields.insert("_id".to_string());

    let mut functions = Vec::new();
    for item in returning {
        match item {
            SelectItem::Wildcard => {}
            SelectItem::Column { name, .. } => {
                if name == "_id" {
                    continue;
                }

                if !schema
                    .fields
                    .iter()
                    .any(|field| field.name.eq_ignore_ascii_case(name))
                {
                    return Err(CassieError::Planner(format!(
                        "{operation} RETURNING column '{name}' does not exist in '{table}'"
                    )));
                }
            }
            SelectItem::Function { function, .. } => {
                validate_expression(
                    &Expr::Function(function.clone()),
                    &known_fields,
                    &HashSet::new(),
                    false,
                )?;
                collect_item(item, &mut functions);
            }
            SelectItem::Expr { expr, .. } => {
                validate_expression(expr, &known_fields, &HashSet::new(), false)?;
            }
            SelectItem::WindowFunction { .. } => {
                return Err(CassieError::Planner(format!(
                    "{operation} RETURNING does not support window functions"
                )));
            }
        }
    }

    validate_function_calls(functions, catalog)
}
