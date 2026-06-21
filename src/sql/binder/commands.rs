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

    if let Some(on_conflict) = &mut statement.on_conflict {
        let mut normalized_target = Vec::with_capacity(on_conflict.target_fields.len());
        for field in &on_conflict.target_fields {
            let field_name = field.trim();
            if field_name.is_empty() {
                return Err(CassieError::Planner(
                    "ON CONFLICT target fields cannot be empty".into(),
                ));
            }
            if !schema
                .fields
                .iter()
                .any(|candidate| candidate.name.eq_ignore_ascii_case(field_name))
            {
                return Err(CassieError::Planner(format!(
                    "ON CONFLICT target column '{field_name}' does not exist in '{table}'"
                )));
            }
            normalized_target.push(field_name.to_string());
        }
        on_conflict.target_fields = normalized_target;

        if matches!(
            on_conflict.action,
            crate::sql::ast::InsertConflictAction::DoUpdate { .. }
        ) && on_conflict.target_fields.is_empty()
        {
            return Err(CassieError::Planner(
                "ON CONFLICT DO UPDATE requires an explicit conflict target".into(),
            ));
        }

        if !on_conflict.target_fields.is_empty()
            && !conflict_target_supported(catalog, &table, &on_conflict.target_fields)
        {
            return Err(CassieError::Planner(format!(
                "ON CONFLICT target {:?} does not match a unique or primary key on '{table}'",
                on_conflict.target_fields
            )));
        }
    }

    if let InsertSource::Select(select) = statement.source {
        let source = bind_select(*select, catalog, &HashMap::new())?;
        statement.source = InsertSource::Select(Box::new(source));
    }

    validate_returning_items(&statement.returning, &schema, &table, "INSERT", catalog)?;

    statement.table = table;
    Ok(statement)
}

fn conflict_target_supported(catalog: &Catalog, table: &str, target_fields: &[String]) -> bool {
    if target_fields.is_empty() {
        return true;
    }

    let normalized_target = target_fields
        .iter()
        .map(|field| field.to_ascii_lowercase())
        .collect::<Vec<_>>();

    let constraint_fields = catalog
        .get_constraints(table)
        .into_iter()
        .filter(|constraint| constraint.primary_key || constraint.unique)
        .map(|constraint| vec![constraint.field.to_ascii_lowercase()])
        .collect::<Vec<_>>();
    if constraint_fields
        .iter()
        .any(|fields| fields.as_slice() == normalized_target.as_slice())
    {
        return true;
    }

    catalog
        .list_indexes(table)
        .into_iter()
        .filter(|index| index.unique && index.kind == crate::catalog::IndexKind::Scalar)
        .map(|index| {
            index
                .normalized_fields()
                .into_iter()
                .map(|field| field.to_ascii_lowercase())
                .collect::<Vec<_>>()
        })
        .any(|fields| fields.as_slice() == normalized_target.as_slice())
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

pub(super) fn bind_create_rollup(
    mut statement: crate::sql::ast::CreateRollupStatement,
    catalog: &Catalog,
) -> Result<crate::sql::ast::CreateRollupStatement, CassieError> {
    let name = statement.name.trim().to_string();
    if name.is_empty() {
        return Err(CassieError::Planner("CREATE ROLLUP requires a name".into()));
    }
    if catalog.get_rollup(&name).is_some() {
        if statement.if_not_exists {
            statement.name = name;
            return Ok(statement);
        }
        return Err(CassieError::Planner(format!(
            "rollup '{name}' already exists"
        )));
    }

    let source = statement.source.trim().to_string();
    if source.is_empty() || !catalog.exists(&source) {
        return Err(CassieError::CollectionNotFound(source));
    }
    if virtual_views::schema(&source).is_some() || catalog.get_view(&source).is_some() {
        return Err(CassieError::Unsupported(format!(
            "rollup source '{source}' must be a base collection"
        )));
    }

    if !statement.bucket.name.eq_ignore_ascii_case("time_bucket") {
        return Err(CassieError::Planner(
            "CREATE ROLLUP USING requires time_bucket".into(),
        ));
    }
    if !(2..=3).contains(&statement.bucket.args.len()) {
        return Err(CassieError::Planner(
            "time_bucket rollups require width, timestamp[, origin]".into(),
        ));
    }
    let Expr::Column(timestamp_field) = &statement.bucket.args[1] else {
        return Err(CassieError::Planner(
            "time_bucket rollup timestamp argument must be a column".into(),
        ));
    };

    let schema = catalog
        .get_schema(&source)
        .ok_or_else(|| CassieError::CollectionNotFound(source.clone()))?;
    let known_fields = schema
        .fields
        .iter()
        .map(|field| field.name.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    if !known_fields.contains(&timestamp_field.to_ascii_lowercase()) {
        return Err(CassieError::Planner(format!(
            "rollup timestamp column '{timestamp_field}' does not exist in '{source}'"
        )));
    }

    for expr in &statement.group_by {
        let Expr::Column(name) = expr else {
            return Err(CassieError::Planner(
                "rollup GROUP BY supports source columns only".into(),
            ));
        };
        if !known_fields.contains(&name.to_ascii_lowercase()) {
            return Err(CassieError::Planner(format!(
                "rollup group column '{name}' does not exist in '{source}'"
            )));
        }
    }

    for item in &statement.aggregates {
        let SelectItem::Function { function, .. } = item else {
            return Err(CassieError::Planner(
                "rollup AGGREGATES supports aggregate functions only".into(),
            ));
        };
        if !matches!(
            function.name.to_ascii_lowercase().as_str(),
            "count" | "sum" | "avg" | "min" | "max"
        ) {
            return Err(CassieError::Unsupported(format!(
                "rollup aggregate '{}' is not supported",
                function.name
            )));
        }
        if !crate::sql::functions::is_aggregate_function(&function.name) {
            return Err(CassieError::Planner(format!(
                "'{}' is not an aggregate function",
                function.name
            )));
        }
        if !(function.name.eq_ignore_ascii_case("count")
            && matches!(function.args.as_slice(), [Expr::Column(name)] if name == "*"))
        {
            for arg in &function.args {
                validate_expression(arg, &known_fields, &HashSet::new(), false)?;
            }
        }
    }

    if let Some(filter) = &statement.filter {
        validate_expression(filter, &known_fields, &HashSet::new(), false)?;
    }

    statement.name = name;
    statement.source = source;
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
