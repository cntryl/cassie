//! Lower a visible alias namespace to existing physical field references.
//!
//! Single-relation lowering preserves Collection access paths. Join wrappers
//! retain the underlying source and supply distinct runtime row qualifiers.

use super::{CassieError, Catalog, CteScope, Expr, QuerySource, SelectItem, SelectStatement};
use crate::sql::ColumnIdentifierPath;

pub(crate) fn qualifier(alias: &str) -> String {
    let name = ColumnIdentifierPath::parse(alias)
        .map_or_else(|_| alias.to_string(), |path| path.declared_name());
    let bytes = name
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("__cassie_relation_alias_{bytes}")
}

/// Ordered visible-to-physical fields, including the existing implicit id.
pub(super) fn columns(
    source: &QuerySource,
    aliases: &[String],
    catalog: &Catalog,
    scope: &CteScope,
) -> Result<Vec<(String, String)>, CassieError> {
    let (names, implicit_identity) = match source {
        QuerySource::Collection(name) => {
            let schema = super::inference::relation_output_schema(catalog, name)?;
            let implicit = catalog.get_view(name).is_none()
                && catalog.get_materialized_projection(name).is_none()
                && catalog
                    .get_schema(name)
                    .is_some_and(|schema| !schema.declares_id());
            (
                schema
                    .fields
                    .into_iter()
                    .map(|field| field.name)
                    .collect::<Vec<_>>(),
                implicit,
            )
        }
        QuerySource::Cte(name) => {
            let binding = scope
                .get(&name.to_ascii_lowercase())
                .ok_or_else(|| CassieError::CollectionNotFound(name.clone()))?;
            if aliases.len() > binding.visible.len() {
                return Err(CassieError::Planner(
                    "relation alias list has more names than source columns".into(),
                ));
            }
            return Ok(binding
                .visible
                .iter()
                .zip(&binding.row_fields)
                .enumerate()
                .map(|(index, (name, row))| {
                    (
                        aliases
                            .get(index)
                            .cloned()
                            .unwrap_or_else(|| ColumnIdentifierPath::stored_field_key(name)),
                        ColumnIdentifierPath::stored_field_key(&row.name),
                    )
                })
                .collect());
        }
        QuerySource::Aliased { source, .. } => return columns(source, aliases, catalog, scope),
        QuerySource::Subquery { select, .. } => (
            super::inference::infer_select_schema(select, catalog)?
                .fields
                .into_iter()
                .map(|field| field.name)
                .collect(),
            false,
        ),
        QuerySource::TableFunction { name, .. } => (
            super::select::table_function_columns(name)
                .into_iter()
                .map(|(name, _)| name)
                .collect(),
            false,
        ),
        QuerySource::SingleRow => (Vec::new(), false),
        _ => {
            return Err(CassieError::Planner(
                "ordinary aliases require a collection or CTE".into(),
            ))
        }
    };
    if aliases.len() > names.len() {
        return Err(CassieError::Planner(
            "relation alias list has more names than source columns".into(),
        ));
    }
    Ok(names
        .into_iter()
        .enumerate()
        .map(|(index, name)| {
            let visible = aliases
                .get(index)
                .cloned()
                .unwrap_or_else(|| ColumnIdentifierPath::stored_field_key(&name));
            let physical = if index == 0 && implicit_identity {
                "_id".to_string()
            } else {
                ColumnIdentifierPath::stored_field_key(&name)
            };
            (visible, physical)
        })
        .collect())
}

struct Namespace {
    name: String,
    qualifier: String,
    fields: Vec<(String, String)>,
    reserved_identity: bool,
    hidden: Vec<String>,
}

fn namespaces(
    source: &QuerySource,
    catalog: &Catalog,
    scope: &CteScope,
    result: &mut Vec<Namespace>,
) -> Result<(), CassieError> {
    match source {
        QuerySource::Aliased {
            source,
            alias,
            column_aliases,
        } => {
            let name = ColumnIdentifierPath::parse(alias)
                .map_err(CassieError::Planner)?
                .declared_name();
            if result.iter().any(|namespace| namespace.name == name) {
                return Err(CassieError::Planner(format!(
                    "duplicate relation alias '{name}'"
                )));
            }
            result.push(Namespace {
                hidden: match source.as_ref() {
                    QuerySource::Collection(name) => crate::catalog::qualifier_variants(name),
                    _ => Vec::new(),
                },
                name,
                qualifier: qualifier(alias),
                fields: columns(source, column_aliases, catalog, scope)?,
                reserved_identity: base_identity(source, catalog),
            });
        }
        QuerySource::Join { left, right, .. } => {
            namespaces(left, catalog, scope, result)?;
            namespaces(right, catalog, scope, result)?;
        }
        QuerySource::Collection(name) => result.push(Namespace {
            name: name.to_string(),
            qualifier: name.to_string(),
            hidden: Vec::new(),
            fields: columns(source, &[], catalog, scope)?,
            reserved_identity: base_identity(source, catalog),
        }),
        QuerySource::Cte(name) | QuerySource::TableFunction { name, .. } => {
            result.push(Namespace {
                name: name.clone(),
                qualifier: name.clone(),
                hidden: Vec::new(),
                fields: columns(source, &[], catalog, scope)?,
                reserved_identity: false,
            })
        }
        QuerySource::Subquery { alias, .. } => result.push(Namespace {
            name: alias.clone(),
            qualifier: alias.clone(),
            hidden: Vec::new(),
            fields: columns(source, &[], catalog, scope)?,
            reserved_identity: false,
        }),
        _ => {}
    }
    Ok(())
}

fn reference(name: &str, namespaces: &[Namespace], single: bool) -> Result<String, CassieError> {
    let path = ColumnIdentifierPath::parse(name).map_err(CassieError::Planner)?;
    let field = path.field_lookup_key();
    if path.is_qualified() && path.namespace_qualifier().is_none() {
        if single {
            return Err(CassieError::Planner(format!(
                "unknown relation qualifier in '{name}'"
            )));
        }
        return Ok(name.to_string());
    }
    if let Some(qualifier) = path.namespace_qualifier() {
        if namespaces
            .iter()
            .any(|namespace| namespace.qualifier == qualifier)
        {
            return Ok(name.to_string());
        }
        let Some(namespace) = namespaces
            .iter()
            .find(|namespace| namespace.name == qualifier)
        else {
            if single
                && namespaces
                    .iter()
                    .any(|space| space.hidden.iter().any(|name| name == &qualifier))
            {
                return Err(CassieError::Planner(format!(
                    "unknown relation qualifier '{qualifier}'"
                )));
            }
            // Existing validation rejects hidden/unknown qualifiers and resolves
            // catalog schema-qualified names by its existing authority.
            return Ok(name.to_string());
        };
        if namespace
            .fields
            .iter()
            .filter(|(visible, _)| *visible == field)
            .count()
            > 1
        {
            return Err(CassieError::Planner(format!("ambiguous column '{name}'")));
        }
        let physical = if field == "_id" && namespace.reserved_identity {
            "_id"
        } else {
            namespace
                .fields
                .iter()
                .find(|(visible, _)| *visible == field)
                .map(|(_, physical)| physical.as_str())
                .ok_or_else(|| CassieError::Planner(format!("unknown column '{name}'")))?
        };
        return Ok(if single {
            physical.to_string()
        } else {
            format!("{}.{}", namespace.qualifier, physical)
        });
    }
    if namespaces.iter().any(|space| {
        space
            .fields
            .iter()
            .filter(|(visible, _)| *visible == field)
            .count()
            > 1
    }) {
        return Err(CassieError::Planner(format!("ambiguous column '{name}'")));
    }
    let mut matches = namespaces.iter().filter_map(|namespace| {
        let physical = if field == "_id" && namespace.reserved_identity {
            Some("_id")
        } else {
            namespace
                .fields
                .iter()
                .find(|(visible, _)| *visible == field)
                .map(|(_, physical)| physical.as_str())
        }?;
        Some((namespace, physical))
    });
    let Some((namespace, physical)) = matches.next() else {
        if namespaces.iter().any(|space| {
            space
                .fields
                .iter()
                .any(|(visible, physical)| *physical == field && *visible != field)
        }) {
            return Err(CassieError::Planner(format!("unknown column '{name}'")));
        }
        return Ok(name.to_string());
    };
    if matches.next().is_some() {
        return Err(CassieError::Planner(format!("ambiguous column '{name}'")));
    }
    Ok(if single {
        physical.to_string()
    } else {
        format!("{}.{}", namespace.qualifier, physical)
    })
}

fn rewrite(expr: &mut Expr, spaces: &[Namespace], single: bool) -> Result<(), CassieError> {
    if let Expr::Exists(statement) = expr {
        nested::rewrite(statement, spaces)?;
        return Ok(());
    }
    if let Expr::Column(name) = expr {
        *name = reference(name, spaces, single)?;
        return Ok(());
    }
    let mut failure = None;
    *expr = expr.map_children(|child| {
        let mut child = child.clone();
        if let Err(error) = rewrite(&mut child, spaces, single) {
            failure = Some(error);
        }
        child
    });
    failure.map_or(Ok(()), Err)
}

pub(super) fn lower_join_on(
    source: &mut QuerySource,
    catalog: &Catalog,
    scope: &CteScope,
) -> Result<(), CassieError> {
    if !has_alias(source) {
        return Ok(());
    }
    let mut spaces = Vec::new();
    namespaces(source, catalog, scope, &mut spaces)?;
    if spaces.is_empty() {
        return Ok(());
    }
    if let QuerySource::Join { on, .. } = source {
        rewrite(on, &spaces, false)?;
    }
    Ok(())
}

pub(super) fn lower_select(
    select: &mut SelectStatement,
    catalog: &Catalog,
    scope: &CteScope,
) -> Result<(), CassieError> {
    if !has_alias(&select.source) {
        return Ok(());
    }
    let mut spaces = Vec::new();
    namespaces(&select.source, catalog, scope, &mut spaces)?;
    if spaces.is_empty() {
        return Ok(());
    }
    let correlated = select.filter.as_ref().is_some_and(|expr| {
        expr.any_descendant_or_self(&mut |expr| matches!(expr, Expr::Exists(_)))
    });
    let single = !correlated
        && matches!(&select.source, QuerySource::Aliased { source, .. }
        if matches!(source.as_ref(), QuerySource::Collection(_)));
    let has_declared_id = spaces
        .iter()
        .flat_map(|space| &space.fields)
        .any(|(_, physical)| {
            ColumnIdentifierPath::parse(physical)
                .is_ok_and(|path| path.final_name().eq_ignore_ascii_case("id"))
        });
    let output_aliases = super::collect_projection_aliases(select);
    let mut projection = Vec::new();
    for mut item in std::mem::take(&mut select.projection) {
        if matches!(item, SelectItem::Wildcard) {
            for space in &spaces {
                for (visible, physical) in &space.fields {
                    if has_declared_id && physical == "_id" {
                        continue;
                    }
                    projection.push(SelectItem::Column {
                        name: if single {
                            physical.clone()
                        } else {
                            format!("{}.{}", space.qualifier, physical)
                        },
                        alias: Some(
                            ColumnIdentifierPath::parse(visible)
                                .map_or_else(|_| visible.clone(), |path| path.declared_name()),
                        ),
                    });
                }
            }
            continue;
        }
        match &mut item {
            SelectItem::Column { name, alias } => {
                let output = ColumnIdentifierPath::parse(name)
                    .map_err(CassieError::Planner)?
                    .declared_name();
                *name = reference(name, &spaces, single)?;
                if alias.is_none() {
                    *alias = Some(output);
                }
            }
            SelectItem::Expr { expr, .. } => rewrite(expr, &spaces, single)?,
            SelectItem::Function { function, .. } => {
                for expr in &mut function.args {
                    rewrite(expr, &spaces, single)?;
                }
            }
            SelectItem::WindowFunction { function, .. } => {
                for expr in function.args.iter_mut().chain(&mut function.partition_by) {
                    rewrite(expr, &spaces, single)?;
                }
                for order in &mut function.order_by {
                    rewrite(&mut order.expr, &spaces, single)?;
                }
            }
            SelectItem::Wildcard => unreachable!(),
        }
        projection.push(item);
    }
    select.projection = projection;
    for expr in select
        .filter
        .iter_mut()
        .chain(&mut select.having)
        .chain(&mut select.group_by)
        .chain(&mut select.distinct_on)
    {
        rewrite(expr, &spaces, single)?;
    }
    for order in &mut select.order {
        if let Expr::Column(name) = &order.expr {
            if ColumnIdentifierPath::parse(name).is_ok_and(|path| !path.is_qualified())
                && output_aliases.contains(&ColumnIdentifierPath::reference_field_key(name))
            {
                continue;
            }
        }
        rewrite(&mut order.expr, &spaces, single)?;
    }
    if single {
        let source = std::mem::replace(&mut select.source, QuerySource::SingleRow);
        if let QuerySource::Aliased { source, .. } = source {
            select.source = *source;
        }
    }
    Ok(())
}

fn has_alias(source: &QuerySource) -> bool {
    match source {
        QuerySource::Aliased { .. } => true,
        QuerySource::Join { left, right, .. } => has_alias(left) || has_alias(right),
        _ => false,
    }
}

fn base_identity(source: &QuerySource, catalog: &Catalog) -> bool {
    let QuerySource::Collection(name) = source else {
        return false;
    };
    catalog.get_schema(name).is_some()
        && catalog.get_view(name).is_none()
        && catalog.get_materialized_projection(name).is_none()
        && crate::catalog::virtual_views::schema(name).is_none()
}

#[path = "alias_nested.rs"]
mod nested;

/// Lower the existing lateral source's qualified references to its left scope.
pub(super) fn lower_lateral(
    left: &QuerySource,
    right: &mut QuerySource,
    catalog: &Catalog,
    scope: &CteScope,
) -> Result<(), CassieError> {
    if !has_alias(left) {
        return Ok(());
    }
    let mut spaces = Vec::new();
    namespaces(left, catalog, scope, &mut spaces)?;
    match right {
        QuerySource::Subquery {
            select,
            lateral: true,
            ..
        } => nested::query(select, &spaces)?,
        QuerySource::TableFunction {
            function,
            lateral: true,
            ..
        } => {
            for expr in &mut function.args {
                rewrite(expr, &spaces, false)?;
            }
        }
        _ => {}
    }
    Ok(())
}
