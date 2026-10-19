//! GROUP BY projection validation, including PostgreSQL primary-key dependencies.

use std::collections::HashSet;

use crate::sql::ast::IdentifierPath;
use crate::sql::ColumnIdentifierPath;

use super::{CassieError, Catalog, CteScope, Expr, QuerySource, SelectItem, SelectStatement};

pub(super) fn validate(
    select: &SelectStatement,
    projection_aliases: &HashSet<String>,
) -> Result<(), CassieError> {
    let group_by = select.group_by.as_slice();
    if group_by.is_empty() && select.having.is_none() && !projection_has_aggregate(select) {
        return Ok(());
    }

    for item in &select.projection {
        let expression = match item {
            SelectItem::Wildcard => {
                return Err(CassieError::Planner(
                    "wildcard projection must appear in the GROUP BY clause".to_string(),
                ));
            }
            SelectItem::Column { name, .. } => Expr::Column(name.clone()),
            SelectItem::Function { function, .. }
                if crate::sql::functions::is_aggregate_function(&function.name) =>
            {
                continue;
            }
            SelectItem::Function { function, .. } => Expr::Function(Box::new(function.clone())),
            SelectItem::Expr { expr, .. } => expr.clone(),
            SelectItem::WindowFunction { .. } => continue,
        };
        reject_ungrouped_column(&expression, group_by)?;
    }
    for order in &select.order {
        let names_output = matches!(&order.expr, Expr::Column(name)
            if names_projection_output(select, projection_aliases, name));
        if !names_output && !matches!(order.expr, Expr::IntegerLiteral(_)) {
            reject_ungrouped_column(&order.expr, group_by)?;
        }
    }
    Ok(())
}

pub(super) fn prepare(
    select: &mut SelectStatement,
    known_fields: &HashSet<String>,
    catalog: &Catalog,
    scope: &CteScope<'_>,
) -> Result<HashSet<String>, CassieError> {
    let projection_aliases = super::collect_projection_aliases(select);
    resolve_group_by_output_references(select, known_fields)?;
    expand_primary_key_dependent_columns(select, &projection_aliases, catalog, scope)?;
    Ok(projection_aliases)
}

fn resolve_group_by_output_references(
    select: &mut SelectStatement,
    known_fields: &HashSet<String>,
) -> Result<(), CassieError> {
    let group_by = std::mem::take(&mut select.group_by);
    select.group_by = group_by
        .into_iter()
        .map(|expression| resolve_group_by_output_reference(expression, select, known_fields))
        .collect::<Result<_, _>>()?;
    Ok(())
}

fn resolve_group_by_output_reference(
    expression: Expr,
    select: &SelectStatement,
    known_fields: &HashSet<String>,
) -> Result<Expr, CassieError> {
    match expression {
        Expr::IntegerLiteral(position) => group_by_output_at(position, &select.projection),
        Expr::Column(name)
            if crate::sql::ColumnIdentifierPath::parse(&name)
                .is_ok_and(|path| !path.is_qualified()) =>
        {
            match super::references::validate_column_reference(
                &name,
                known_fields,
                &HashSet::new(),
                false,
            ) {
                Ok(()) => return Ok(Expr::Column(name)),
                Err(CassieError::UndefinedColumn(_)) => {}
                Err(error) => return Err(error),
            }

            let alias_key = crate::sql::ColumnIdentifierPath::reference_field_key(&name);
            let mut matching_items = select.projection.iter().filter(|item| {
                projection_alias(item).is_some_and(|alias| {
                    crate::sql::ColumnIdentifierPath::stored_field_key(alias) == alias_key
                })
            });
            let Some(item) = matching_items.next() else {
                return Ok(Expr::Column(name));
            };
            if matching_items.next().is_some() {
                return Err(CassieError::AmbiguousColumn(name));
            }
            group_by_projection_expression(item)
        }
        expression => Ok(expression),
    }
}

fn group_by_output_at(position: i64, projection: &[SelectItem]) -> Result<Expr, CassieError> {
    let invalid_position = || {
        CassieError::Planner(format!(
            "GROUP BY position {position} is not in select list"
        ))
    };
    let index = usize::try_from(position)
        .ok()
        .and_then(|position| position.checked_sub(1))
        .ok_or_else(invalid_position)?;
    if projection
        .iter()
        .take(index)
        .any(|item| matches!(item, SelectItem::Wildcard))
    {
        return Err(CassieError::Planner(
            "GROUP BY position is not supported with SELECT *".to_string(),
        ));
    }
    let item = projection.get(index).ok_or_else(invalid_position)?;
    group_by_projection_expression(item)
}

fn group_by_projection_expression(item: &SelectItem) -> Result<Expr, CassieError> {
    let expression = match item {
        SelectItem::Column { name, .. } => Expr::Column(name.clone()),
        SelectItem::Function { function, .. } => Expr::Function(Box::new(function.clone())),
        SelectItem::Expr { expr, .. } => expr.clone(),
        SelectItem::WindowFunction { .. } => {
            return Err(CassieError::Planner(
                "window functions are not allowed in GROUP BY".to_string(),
            ));
        }
        SelectItem::Wildcard => {
            return Err(CassieError::Planner(
                "GROUP BY position is not supported with SELECT *".to_string(),
            ));
        }
    };
    if expression.any_descendant_or_self(&mut |node| {
        matches!(node, Expr::Function(function) if crate::sql::functions::is_aggregate_function(&function.name))
    }) {
        return Err(CassieError::Planner(
            "aggregate functions are not allowed in GROUP BY".to_string(),
        ));
    }
    Ok(expression)
}

fn projection_alias(item: &SelectItem) -> Option<&str> {
    match item {
        SelectItem::Column { alias, .. }
        | SelectItem::Function { alias, .. }
        | SelectItem::Expr { alias, .. }
        | SelectItem::WindowFunction { alias, .. } => alias.as_deref(),
        SelectItem::Wildcard => None,
    }
}

fn reject_ungrouped_column(expr: &Expr, group_by: &[Expr]) -> Result<(), CassieError> {
    match first_ungrouped_column(expr, group_by) {
        Some(column) => Err(CassieError::Planner(format!(
            "column '{column}' must appear in the GROUP BY clause or be used in an aggregate function"
        ))),
        None => Ok(()),
    }
}

fn first_ungrouped_column<'a>(expr: &'a Expr, group_by: &[Expr]) -> Option<&'a str> {
    if group_by.iter().any(|grouped| grouped.structurally_eq(expr)) {
        return None;
    }
    match expr {
        Expr::Column(column) => Some(column),
        Expr::Function(function)
            if crate::sql::functions::is_aggregate_function(&function.name) =>
        {
            None
        }
        _ => expr.find_map_child(|child| first_ungrouped_column(child, group_by)),
    }
}

pub(super) fn expand_primary_key_dependent_columns(
    select: &mut SelectStatement,
    projection_aliases: &HashSet<String>,
    catalog: &Catalog,
    scope: &CteScope<'_>,
) -> Result<(), CassieError> {
    if select.group_by.is_empty() {
        return Ok(());
    }
    let dependencies = primary_key_dependent_columns(select, catalog, scope)?;
    if dependencies.is_empty() {
        return Ok(());
    }

    let mut additional_group_columns = Vec::new();
    for item in &select.projection {
        match item {
            SelectItem::Column { name, .. } => collect_dependent_columns(
                &Expr::Column(name.clone()),
                &select.group_by,
                &dependencies,
                &mut additional_group_columns,
            ),
            SelectItem::Function { function, .. }
                if crate::sql::functions::is_aggregate_function(&function.name) => {}
            SelectItem::Function { function, .. } => collect_dependent_columns(
                &Expr::Function(Box::new(function.clone())),
                &select.group_by,
                &dependencies,
                &mut additional_group_columns,
            ),
            SelectItem::Expr { expr, .. } => collect_dependent_columns(
                expr,
                &select.group_by,
                &dependencies,
                &mut additional_group_columns,
            ),
            SelectItem::Wildcard | SelectItem::WindowFunction { .. } => {}
        }
    }
    if let Some(having) = &select.having {
        collect_dependent_columns(
            having,
            &select.group_by,
            &dependencies,
            &mut additional_group_columns,
        );
    }
    for order in &select.order {
        let names_output = matches!(&order.expr, Expr::Column(name)
            if names_projection_output(select, projection_aliases, name));
        if !names_output && !matches!(order.expr, Expr::IntegerLiteral(_)) {
            collect_dependent_columns(
                &order.expr,
                &select.group_by,
                &dependencies,
                &mut additional_group_columns,
            );
        }
    }

    for column in additional_group_columns {
        if !select
            .group_by
            .iter()
            .any(|grouped| grouped.structurally_eq(&column))
        {
            select.group_by.push(column);
        }
    }
    Ok(())
}

fn collect_dependent_columns(
    expr: &Expr,
    group_by: &[Expr],
    dependencies: &HashSet<String>,
    columns: &mut Vec<Expr>,
) {
    if group_by.iter().any(|grouped| grouped.structurally_eq(expr)) {
        return;
    }
    match expr {
        Expr::Column(column) if dependencies.contains(&column_reference_key(column)) => {
            columns.push(Expr::Column(column.clone()));
        }
        Expr::Function(function)
            if crate::sql::functions::is_aggregate_function(&function.name) => {}
        _ => expr.for_each_child(|child| {
            collect_dependent_columns(child, group_by, dependencies, columns);
        }),
    }
}

fn primary_key_dependent_columns(
    select: &SelectStatement,
    catalog: &Catalog,
    scope: &CteScope<'_>,
) -> Result<HashSet<String>, CassieError> {
    let grouped_columns = select
        .group_by
        .iter()
        .filter_map(|expression| match expression {
            Expr::Column(column) => Some(column_reference_key(column)),
            _ => None,
        })
        .collect::<HashSet<_>>();
    if grouped_columns.is_empty() {
        return Ok(HashSet::new());
    }

    let mut relations = Vec::new();
    collect_base_relations(&select.source, &mut relations);
    let mut dependent_columns = HashSet::new();
    for relation in relations {
        if catalog.get_schema(relation.table.as_str()).is_none()
            || catalog.get_view(relation.table.as_str()).is_some()
            || catalog
                .get_materialized_projection(relation.table.as_str())
                .is_some()
            || super::virtual_views::schema(relation.table.as_str()).is_some()
        {
            continue;
        }

        let constraints = catalog.get_constraints(relation.table.as_str());
        let primary_key_constraints = constraints
            .iter()
            .filter(|constraint| constraint.primary_key)
            .collect::<Vec<_>>();
        if primary_key_constraints.is_empty()
            || primary_key_constraints
                .iter()
                .map(|constraint| {
                    constraint
                        .primary_key_name
                        .as_ref()
                        .map(|name| name.to_ascii_lowercase())
                })
                .collect::<HashSet<_>>()
                .len()
                != 1
        {
            continue;
        }
        let primary_key_fields = primary_key_constraints
            .iter()
            .map(|constraint| constraint.field.as_str())
            .collect::<Vec<_>>();
        let unique_primary_key_fields = primary_key_fields
            .iter()
            .map(|field| ColumnIdentifierPath::stored_field_key(field))
            .collect::<HashSet<_>>();
        let unique_primary_key_ordinals = primary_key_constraints
            .iter()
            .filter_map(|constraint| constraint.primary_key_ordinal)
            .collect::<HashSet<_>>();
        if unique_primary_key_fields.len() != primary_key_fields.len()
            || (primary_key_constraints.len() > 1
                && (unique_primary_key_ordinals.len() != primary_key_constraints.len()
                    || primary_key_constraints
                        .iter()
                        .any(|constraint| constraint.primary_key_ordinal.is_none())))
        {
            continue;
        }

        let source = QuerySource::Collection(relation.table.clone());
        let visible_fields =
            super::aliases::columns(&source, relation.column_aliases, catalog, scope)?;
        let key_columns = primary_key_fields
            .iter()
            .map(|primary_key_field| unique_physical_column(&visible_fields, primary_key_field))
            .collect::<Option<Vec<_>>>();
        let Some(key_columns) = key_columns else {
            continue;
        };
        let qualifiers = relation.qualifiers();
        if !key_columns.iter().all(|field| {
            column_reference_keys(&qualifiers, field)
                .iter()
                .any(|key| grouped_columns.contains(key))
        }) {
            continue;
        }

        for (_, physical) in visible_fields {
            dependent_columns.extend(column_reference_keys(&qualifiers, &physical));
        }
    }
    Ok(dependent_columns)
}

fn unique_physical_column<'a>(
    visible_fields: &'a [(String, String)],
    physical_field: &str,
) -> Option<&'a str> {
    let physical_key = ColumnIdentifierPath::stored_field_key(physical_field);
    let mut matches = visible_fields
        .iter()
        .filter(|(_, physical)| ColumnIdentifierPath::stored_field_key(physical) == physical_key)
        .map(|(_, physical)| physical.as_str());
    let physical = matches.next()?;
    matches.next().is_none().then_some(physical)
}

fn column_reference_keys(qualifiers: &[String], field: &str) -> Vec<String> {
    let mut keys = vec![ColumnIdentifierPath::stored_field_key(field)];
    let escaped_field = field.replace('"', "\"\"");
    keys.extend(qualifiers.iter().filter_map(|qualifier| {
        ColumnIdentifierPath::parse(&format!("{qualifier}.\"{escaped_field}\""))
            .ok()
            .map(|path| path.lookup_key())
    }));
    keys
}

fn column_reference_key(column: &str) -> String {
    ColumnIdentifierPath::parse(column)
        .map_or_else(|_| column.to_ascii_lowercase(), |path| path.lookup_key())
}

struct BaseRelation<'a> {
    table: &'a IdentifierPath,
    alias: Option<&'a str>,
    column_aliases: &'a [String],
}

impl BaseRelation<'_> {
    fn qualifiers(&self) -> Vec<String> {
        self.alias.map_or_else(
            || crate::catalog::qualifier_variants(self.table.as_str()),
            |alias| vec![super::aliases::qualifier(alias)],
        )
    }
}

fn collect_base_relations<'a>(source: &'a QuerySource, relations: &mut Vec<BaseRelation<'a>>) {
    match source {
        QuerySource::Collection(table) => relations.push(BaseRelation {
            table,
            alias: None,
            column_aliases: &[],
        }),
        QuerySource::Aliased {
            source,
            alias,
            column_aliases,
        } => {
            if let QuerySource::Collection(table) = source.as_ref() {
                relations.push(BaseRelation {
                    table,
                    alias: Some(alias),
                    column_aliases,
                });
            }
        }
        QuerySource::Join { left, right, .. } => {
            collect_base_relations(left, relations);
            collect_base_relations(right, relations);
        }
        QuerySource::Values { .. }
        | QuerySource::Cte(_)
        | QuerySource::TableFunction { .. }
        | QuerySource::Subquery { .. }
        | QuerySource::SingleRow => {}
    }
}

/// Whether ORDER BY `name` refers to an output column (an alias or an
/// unaliased function's name) rather than an input column.
fn names_projection_output(
    select: &SelectStatement,
    projection_aliases: &HashSet<String>,
    name: &str,
) -> bool {
    projection_aliases.contains(&ColumnIdentifierPath::reference_field_key(name))
        || select.projection.iter().any(|item| {
            matches!(item, SelectItem::Function { function, alias: None }
                if ColumnIdentifierPath::stored_field_key(&function.name)
                    == ColumnIdentifierPath::reference_field_key(name))
        })
}

/// Whether a projection item calls an aggregate outside a window function.
fn projection_has_aggregate(select: &SelectStatement) -> bool {
    let is_aggregate = |expr: &Expr| {
        expr.any_descendant_or_self(&mut |expr| {
            matches!(expr, Expr::Function(function)
                if crate::sql::functions::is_aggregate_function(&function.name))
        })
    };
    select.projection.iter().any(|item| match item {
        SelectItem::Function { function, .. } => {
            crate::sql::functions::is_aggregate_function(&function.name)
                || function.expressions().any(is_aggregate)
        }
        SelectItem::Expr { expr, .. } => is_aggregate(expr),
        SelectItem::Wildcard | SelectItem::Column { .. } | SelectItem::WindowFunction { .. } => {
            false
        }
    })
}
