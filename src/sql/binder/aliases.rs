//! Lower a visible alias namespace to existing physical field references.
//!
//! Single-relation lowering preserves Collection access paths. Join wrappers
//! retain the underlying source and supply distinct runtime row qualifiers.

use super::{CassieError, Catalog, CteScope, Expr, QuerySource, SelectItem, SelectStatement};
use crate::sql::ast::{RowComparison, RowComparisonOp};
use crate::sql::ColumnIdentifierPath;
use crate::types::Schema;
use std::fmt::Write;

pub(crate) fn qualifier(alias: &str) -> String {
    let name = ColumnIdentifierPath::parse(alias)
        .map_or_else(|_| alias.to_string(), |path| path.declared_name());
    let bytes = name
        .as_bytes()
        .iter()
        .fold(String::new(), |mut bytes, byte| {
            write!(bytes, "{byte:02x}").expect("formatting into a String cannot fail");
            bytes
        });
    format!("__cassie_relation_alias_{bytes}")
}

/// Ordered visible-to-physical fields, including the existing implicit id.
pub(super) fn columns(
    source: &QuerySource,
    aliases: &[String],
    catalog: &Catalog,
    scope: &CteScope<'_>,
) -> Result<Vec<(String, String)>, CassieError> {
    let (names, implicit_identity) = match source {
        QuerySource::Values { rows } => (super::values::names(rows).collect(), false),

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
            let identity_alias = aliases.is_empty() && binding.has_implicit_identity();
            return Ok(binding
                .visible
                .iter()
                .zip(&binding.row_fields)
                .enumerate()
                .map(|(index, (name, row))| {
                    (
                        aliases.get(index).cloned().unwrap_or_else(|| {
                            let visible = if identity_alias && name == "_id" {
                                "id"
                            } else {
                                name
                            };
                            ColumnIdentifierPath::stored_field_key(visible)
                        }),
                        ColumnIdentifierPath::stored_field_key(&row.name),
                    )
                })
                .collect());
        }
        QuerySource::Aliased { source, .. } | QuerySource::TableSample { source, .. } => {
            return columns(source, aliases, catalog, scope);
        }
        QuerySource::Subquery { select, .. } => (subquery_names(select, catalog, scope)?, false),
        QuerySource::TableFunction { name, .. } => (
            super::select::table_function_columns(name)
                .into_iter()
                .map(|(name, _)| name)
                .collect(),
            false,
        ),
        QuerySource::SingleRow => (Vec::new(), false),
        QuerySource::Join { .. } => {
            return Err(CassieError::Planner(
                "ordinary aliases require a collection or CTE".into(),
            ));
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
    qualifier_only: bool,
}

fn namespaces(
    source: &QuerySource,
    catalog: &Catalog,
    scope: &CteScope<'_>,
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
                qualifier_only: false,
                hidden: match source.as_ref() {
                    QuerySource::Collection(name) => crate::catalog::qualifier_variants(name),
                    _ => Vec::new(),
                },
                name,
                qualifier: qualifier(alias),
                fields: columns(source, column_aliases, catalog, scope)?,
                reserved_identity: base_identity(source, catalog, scope),
            });
        }
        QuerySource::Join { left, right, .. } => {
            namespaces(left, catalog, scope, result)?;
            namespaces(right, catalog, scope, result)?;
        }
        QuerySource::TableSample { source, .. } => namespaces(source, catalog, scope, result)?,
        QuerySource::Collection(name) => result.push(Namespace {
            qualifier_only: false,
            name: name.to_string(),
            qualifier: name.to_string(),
            hidden: Vec::new(),
            fields: columns(source, &[], catalog, scope)?,
            reserved_identity: base_identity(source, catalog, scope),
        }),
        QuerySource::Cte(name) | QuerySource::TableFunction { name, .. } => {
            result.push(Namespace {
                qualifier_only: false,
                name: name.clone(),
                qualifier: name.clone(),
                hidden: Vec::new(),
                fields: columns(source, &[], catalog, scope)?,
                reserved_identity: base_identity(source, catalog, scope),
            });
        }
        QuerySource::Subquery { alias, .. } => result.push(Namespace {
            qualifier_only: false,
            name: alias.clone(),
            qualifier: alias.clone(),
            hidden: Vec::new(),
            fields: columns(source, &[], catalog, scope)?,
            reserved_identity: false,
        }),
        QuerySource::Values { .. } | QuerySource::SingleRow => {}
    }
    Ok(())
}

fn ambiguous_field(namespace: &Namespace, field: &str) -> bool {
    namespace
        .fields
        .iter()
        .filter(|(visible, _)| visible == field)
        .count()
        > 1
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
        if namespace.qualifier_only {
            return Ok(format!("{}.{field}", namespace.qualifier));
        }
        if ambiguous_field(namespace, &field) {
            return Err(CassieError::AmbiguousColumn(name.to_string()));
        }
        let physical = if field == "_id" && namespace.reserved_identity {
            "_id"
        } else {
            namespace
                .fields
                .iter()
                .find(|(visible, _)| *visible == field)
                .map(|(_, physical)| physical.as_str())
                .ok_or_else(|| CassieError::UndefinedColumn(name.to_string()))?
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
        return Err(CassieError::AmbiguousColumn(name.to_string()));
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
            return Err(CassieError::UndefinedColumn(name.to_string()));
        }
        return Ok(name.to_string());
    };
    if matches.next().is_some() {
        return Err(CassieError::AmbiguousColumn(name.to_string()));
    }
    Ok(if single {
        physical.to_string()
    } else {
        format!("{}.{}", namespace.qualifier, physical)
    })
}

fn rewrite(expr: &mut Expr, spaces: &[Namespace], single: bool) -> Result<(), CassieError> {
    rewrite_with_outer(expr, spaces, single, &std::collections::HashSet::new())
}

fn reference_with_outer(
    name: &str,
    spaces: &[Namespace],
    single: bool,
    lateral_fields: &std::collections::HashSet<String>,
) -> Result<String, CassieError> {
    let path = ColumnIdentifierPath::parse(name).map_err(CassieError::Planner)?;
    if let Some(qualifier) = path.namespace_qualifier() {
        let claimed_here = spaces
            .iter()
            .any(|space| space.name == qualifier || space.qualifier == qualifier);
        if !claimed_here && lateral_fields.contains(&path.lookup_key()) {
            return Ok(name.to_string());
        }
    }
    reference(name, spaces, single)
}

fn rewrite_with_outer(
    expr: &mut Expr,
    spaces: &[Namespace],
    single: bool,
    lateral_fields: &std::collections::HashSet<String>,
) -> Result<(), CassieError> {
    if let Expr::Exists(statement) = expr {
        nested::rewrite(statement, spaces)?;
        return Ok(());
    }
    if let Expr::Subquery(subquery) = expr {
        nested::rewrite(&mut subquery.query, spaces)?;
    }
    if let Expr::Column(name) = expr {
        if let Some(fields) = relation_row_fields(name, spaces, single, lateral_fields)? {
            *expr = Expr::Function(Box::new(crate::sql::ast::FunctionCall {
                name: "row".into(),
                args: fields,
                ..crate::sql::ast::FunctionCall::default()
            }));
            return Ok(());
        }
        *name = reference_with_outer(name, spaces, single, lateral_fields)?;
        return Ok(());
    }
    if let Expr::RowCompare(comparison) = expr {
        expand_row_fields(&mut comparison.left, spaces, single, lateral_fields)?;
        expand_row_fields(&mut comparison.right, spaces, single, lateral_fields)?;
        return Ok(());
    }
    if let Expr::Binary { left, op, right } = expr {
        if let (Expr::Column(left_name), Expr::Column(right_name)) = (left.as_ref(), right.as_ref())
        {
            if let (Some(left_fields), Some(right_fields)) = (
                relation_row_fields(left_name, spaces, single, lateral_fields)?,
                relation_row_fields(right_name, spaces, single, lateral_fields)?,
            ) {
                if let Some(op) = RowComparisonOp::from_binary(op) {
                    *expr = Expr::RowCompare(Box::new(RowComparison {
                        left: left_fields,
                        op,
                        right: right_fields,
                    }));
                    return Ok(());
                }
            }
        }
    }
    let mut failure = None;
    *expr = expr.map_children(|child| {
        let mut child = child.clone();
        if let Err(error) = rewrite_with_outer(&mut child, spaces, single, lateral_fields) {
            failure = Some(error);
        }
        child
    });
    failure.map_or(Ok(()), Err)
}

fn relation_row_fields(
    name: &str,
    spaces: &[Namespace],
    single: bool,
    lateral_fields: &std::collections::HashSet<String>,
) -> Result<Option<Vec<Expr>>, CassieError> {
    let Some(space) = spaces
        .iter()
        .find(|space| space.name == name || space.qualifier == name)
    else {
        return Ok(None);
    };
    let fields = space
        .fields
        .iter()
        .map(|(visible, _)| {
            let qualified = format!("{}.{visible}", space.name);
            reference_with_outer(&qualified, spaces, single, lateral_fields).map(Expr::Column)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Some(fields))
}

fn expand_row_fields(
    fields: &mut Vec<Expr>,
    spaces: &[Namespace],
    single: bool,
    lateral_fields: &std::collections::HashSet<String>,
) -> Result<(), CassieError> {
    let mut expanded = Vec::with_capacity(fields.len());
    for field in fields.drain(..) {
        if let Expr::Column(name) = &field {
            if let Some(relation) = name.strip_suffix(".*") {
                let Some(row_fields) =
                    relation_row_fields(relation, spaces, single, lateral_fields)?
                else {
                    return Err(CassieError::Planner(format!(
                        "unknown relation qualifier '{relation}'"
                    )));
                };
                expanded.extend(row_fields);
                continue;
            }
        }
        let mut field = field;
        rewrite_with_outer(&mut field, spaces, single, lateral_fields)?;
        expanded.push(field);
    }
    *fields = expanded;
    Ok(())
}

pub(super) fn lower_join_on(
    source: &mut QuerySource,
    catalog: &Catalog,
    scope: &CteScope<'_>,
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
    scope: &CteScope<'_>,
    lateral_fields: &std::collections::HashSet<String>,
    set_result: bool,
) -> Result<(), CassieError> {
    let has_qualified_wildcard = select
        .projection
        .iter()
        .any(|item| qualified_wildcard_name(item).is_some());
    if !has_alias(&select.source) && !has_qualified_wildcard {
        return Ok(());
    }
    let mut spaces = Vec::new();
    namespaces(&select.source, catalog, scope, &mut spaces)?;
    if spaces.is_empty() {
        if let Some(name) = select.projection.iter().find_map(qualified_wildcard_name) {
            return Err(unknown_wildcard_qualifier(name));
        }
        return Ok(());
    }
    let correlated = select_contains_exists(select);
    let mut output_aliases = super::collect_projection_aliases(select);
    // Reusing the collection's physical qualifier would hide an admitted
    // enclosing reference to that same collection. Retain only this carrier.
    let outer_source_collision = lateral_fields.iter().any(|name| {
        ColumnIdentifierPath::parse(name)
            .ok()
            .and_then(|path| path.namespace_qualifier())
            .is_some_and(|qualifier| spaces.iter().any(|space| space.hidden.contains(&qualifier)))
    });
    let single = !correlated
        && !outer_source_collision
        && !qualified_order_collision(select, &spaces, &output_aliases)
        && matches!(&select.source, QuerySource::Aliased { source, .. }
        if matches!(source.as_ref(), QuerySource::Collection(_)));
    let has_declared_id = spaces
        .iter()
        .flat_map(|space| &space.fields)
        .any(|(_, physical)| {
            ColumnIdentifierPath::parse(physical)
                .is_ok_and(|path| path.final_name().eq_ignore_ascii_case("id"))
        });
    let mut projection = Vec::new();
    for mut item in std::mem::take(&mut select.projection) {
        if expand_wildcard_item(&item, &mut projection, &spaces, single, has_declared_id)? {
            continue;
        }
        match &mut item {
            SelectItem::Column { name, alias } => {
                let output = ColumnIdentifierPath::parse(name)
                    .map_err(CassieError::Planner)?
                    .declared_name();
                *name = reference_with_outer(name, &spaces, single, lateral_fields)?;
                if alias.is_none() {
                    *alias = Some(output);
                }
            }
            SelectItem::Expr { expr, .. } => {
                rewrite_with_outer(expr, &spaces, single, lateral_fields)?;
            }
            SelectItem::Function { function, .. } => {
                for expr in function.expressions_mut() {
                    rewrite_with_outer(expr, &spaces, single, lateral_fields)?;
                }
            }
            SelectItem::WindowFunction { function, .. } => {
                for expr in function.args.iter_mut().chain(&mut function.partition_by) {
                    rewrite_with_outer(expr, &spaces, single, lateral_fields)?;
                }
                for order in &mut function.order_by {
                    rewrite_with_outer(&mut order.expr, &spaces, single, lateral_fields)?;
                }
            }
            SelectItem::Wildcard => unreachable!(),
        }
        projection.push(item);
    }
    select.projection = projection;
    if set_result {
        // Global set ordering addresses exported result names, after projection.
        output_aliases.extend(super::collect_projection_aliases(select));
    }
    rewrite_source_clauses(select, &spaces, single, lateral_fields)?;
    for order in &mut select.order {
        if let Expr::Column(name) = &order.expr {
            if ColumnIdentifierPath::parse(name).is_ok_and(|path| !path.is_qualified())
                && output_aliases.contains(&ColumnIdentifierPath::reference_field_key(name))
            {
                continue;
            }
        }
        rewrite_with_outer(&mut order.expr, &spaces, single, lateral_fields)?;
    }
    if single {
        let source = std::mem::replace(&mut select.source, QuerySource::SingleRow);
        if let QuerySource::Aliased { source, .. } = source {
            select.source = *source;
        }
    }
    Ok(())
}

fn qualified_wildcard_namespace<'a>(
    name: &str,
    spaces: &'a [Namespace],
) -> Result<&'a Namespace, CassieError> {
    let path = ColumnIdentifierPath::parse(name).map_err(CassieError::Planner)?;
    let rendered_qualifier = name.strip_suffix(".*").unwrap_or(name);
    let qualifier = path.namespace_qualifier();
    spaces
        .iter()
        .find(|space| {
            qualifier
                .as_deref()
                .is_some_and(|qualifier| space.name == qualifier || space.qualifier == qualifier)
                || space.name == rendered_qualifier
                || space.qualifier == rendered_qualifier
        })
        .ok_or_else(|| {
            let qualifier = qualifier.unwrap_or_else(|| rendered_qualifier.to_string());
            CassieError::Planner(format!("unknown relation qualifier '{qualifier}'"))
        })
}

fn unknown_wildcard_qualifier(name: &str) -> CassieError {
    let qualifier = name.strip_suffix(".*").unwrap_or(name);
    CassieError::Planner(format!("unknown relation qualifier '{qualifier}'"))
}

fn rewrite_source_clauses(
    select: &mut SelectStatement,
    spaces: &[Namespace],
    single: bool,
    lateral_fields: &std::collections::HashSet<String>,
) -> Result<(), CassieError> {
    for expr in select
        .filter
        .iter_mut()
        .chain(&mut select.having)
        .chain(&mut select.group_by)
        .chain(&mut select.distinct_on)
        .chain(&mut select.limit)
        .chain(&mut select.offset)
    {
        rewrite_with_outer(expr, spaces, single, lateral_fields)?;
    }
    Ok(())
}

fn select_contains_exists(select: &SelectStatement) -> bool {
    let has_exists = |expr: &Expr| expr.any_descendant_or_self(&mut Expr::is_subquery_node);
    select.filter.as_ref().is_some_and(has_exists)
        || select.having.as_ref().is_some_and(has_exists)
        || select.order.iter().any(|order| has_exists(&order.expr))
        || select.projection.iter().any(|item| match item {
            SelectItem::Expr { expr, .. } => has_exists(expr),
            SelectItem::Function { function, .. } => function.expressions().any(has_exists),
            SelectItem::WindowFunction { function, .. } => {
                function
                    .args
                    .iter()
                    .chain(&function.partition_by)
                    .any(has_exists)
                    || function
                        .order_by
                        .iter()
                        .any(|order| has_exists(&order.expr))
            }
            _ => false,
        })
}

fn has_alias(source: &QuerySource) -> bool {
    match source {
        QuerySource::Aliased { .. } => true,
        QuerySource::Join { left, right, .. } => has_alias(left) || has_alias(right),
        _ => false,
    }
}

fn base_identity(source: &QuerySource, catalog: &Catalog, scope: &CteScope<'_>) -> bool {
    if let QuerySource::Cte(name) = source {
        return scope
            .get(&name.to_ascii_lowercase())
            .is_some_and(super::CteBinding::has_implicit_identity);
    }
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
#[path = "alias_scope.rs"]
mod scope;
pub(super) fn allocate_scope(select: &mut SelectStatement) -> Result<(), CassieError> {
    scope::allocate(select)
}

/// Lower the existing lateral source's qualified references to its left scope.
pub(super) fn lower_lateral(
    left: &QuerySource,
    right: &mut QuerySource,
    catalog: &Catalog,
    scope: &CteScope<'_>,
) -> Result<(), CassieError> {
    if !has_alias(left) {
        return Ok(());
    }
    let mut spaces = Vec::new();
    namespaces(left, catalog, scope, &mut spaces)?;
    match right {
        QuerySource::Aliased { source, .. } => lower_lateral(left, source, catalog, scope)?,
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
            for expr in function.expressions_mut() {
                rewrite(expr, &spaces, false)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Keep a source namespace when physical lowering would erase ORDER provenance.
fn qualified_order_collision(
    select: &SelectStatement,
    spaces: &[Namespace],
    output_aliases: &super::HashSet<String>,
) -> bool {
    select.order.iter().any(|order| {
        let Expr::Column(name) = &order.expr else {
            return false;
        };
        ColumnIdentifierPath::parse(name).is_ok_and(|path| path.is_qualified())
            && reference(name, spaces, true).is_ok_and(|physical| {
                output_aliases.contains(&ColumnIdentifierPath::reference_field_key(&physical))
            })
    })
}

fn expand_wildcard(
    projection: &mut Vec<SelectItem>,
    spaces: &[Namespace],
    single: bool,
    has_declared_id: bool,
) {
    for space in spaces {
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
}

fn qualified_wildcard_name(item: &SelectItem) -> Option<&str> {
    match item {
        SelectItem::Column { name, .. } if ColumnIdentifierPath::is_qualified_wildcard(name) => {
            Some(name)
        }
        _ => None,
    }
}

fn expand_wildcard_item(
    item: &SelectItem,
    projection: &mut Vec<SelectItem>,
    spaces: &[Namespace],
    single: bool,
    has_declared_id: bool,
) -> Result<bool, CassieError> {
    if matches!(item, SelectItem::Wildcard) {
        expand_wildcard(projection, spaces, single, has_declared_id);
        return Ok(true);
    }
    let Some(name) = qualified_wildcard_name(item) else {
        return Ok(false);
    };
    let namespace = qualified_wildcard_namespace(name, spaces)?;
    expand_wildcard(
        projection,
        std::slice::from_ref(namespace),
        single,
        has_declared_id,
    );
    Ok(true)
}

pub(super) fn qualified_wildcard_projection_items(
    name: &str,
    source: &QuerySource,
    catalog: &Catalog,
    scope: &CteScope<'_>,
    schema: &Schema,
) -> Result<Vec<SelectItem>, CassieError> {
    let mut spaces = Vec::new();
    namespaces(source, catalog, scope, &mut spaces)?;
    let namespace = qualified_wildcard_namespace(name, &spaces)?;
    let has_declared_id = spaces.iter().any(|space| {
        space.fields.iter().any(|(_, physical)| {
            ColumnIdentifierPath::parse(physical)
                .is_ok_and(|path| path.final_name().eq_ignore_ascii_case("id"))
        })
    });
    let mut projection = Vec::new();
    for (visible, physical) in &namespace.fields {
        if has_declared_id && physical == "_id" {
            continue;
        }
        let qualified = format!("{}.{}", namespace.qualifier, physical);
        let has_qualified_field = schema.fields.iter().any(|field| {
            ColumnIdentifierPath::reference_field_key(&field.name)
                == ColumnIdentifierPath::reference_field_key(&qualified)
        });
        projection.push(SelectItem::Column {
            name: if has_qualified_field {
                qualified
            } else {
                physical.clone()
            },
            alias: Some(
                ColumnIdentifierPath::parse(visible)
                    .map_or_else(|_| visible.clone(), |path| path.declared_name()),
            ),
        });
    }
    Ok(projection)
}

fn subquery_names(
    select: &SelectStatement,
    catalog: &Catalog,
    scope: &CteScope<'_>,
) -> Result<Vec<String>, CassieError> {
    let schemas = scope
        .iter()
        .map(|(name, binding)| {
            (
                name.clone(),
                crate::types::Schema {
                    fields: binding.row_fields.clone(),
                },
            )
        })
        .collect();
    Ok(
        super::inference::infer_select_schema_with_ctes(select, catalog, &schemas)?
            .fields
            .into_iter()
            .map(|field| field.name)
            .collect(),
    )
}
