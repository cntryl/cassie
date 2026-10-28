//! Allocate collision-free private row carriers before source references lower.
use super::{
    nested, qualifier, CassieError, ColumnIdentifierPath, Expr, Namespace, QuerySource, SelectItem,
    SelectStatement,
};
use crate::sql::ast::{CteQuery, QueryStatement};
use std::collections::{HashMap, HashSet};

pub(super) fn allocate(select: &mut SelectStatement) -> Result<(), CassieError> {
    let mut declared = Vec::new();
    let mut visible = HashSet::new();
    scope_names(&select.source, &mut declared, &mut visible);
    let mut occupied = HashSet::new();
    query_names(select, &mut occupied);
    if !declared
        .iter()
        .any(|(_, alias)| occupied.contains(&qualifier(alias)))
    {
        return Ok(());
    }
    let mut spaces = Vec::new();
    let mut alias_names = HashSet::new();
    for (name, _) in &declared {
        if !alias_names.insert(name.clone()) {
            return Err(CassieError::Planner(format!(
                "duplicate relation alias '{name}'"
            )));
        }
    }
    let mut allocated = HashMap::new();
    for (name, alias) in declared {
        if !occupied.contains(&qualifier(&alias)) {
            continue;
        }
        let mut counter = 0_usize;
        let carrier = loop {
            let candidate = format!("__cassie_bound_alias_{counter}");
            let row_key = qualifier(&candidate);
            if !occupied.contains(&candidate) && !occupied.contains(&row_key) {
                occupied.insert(candidate.clone());
                occupied.insert(row_key);
                break candidate;
            }
            counter = counter.checked_add(1).ok_or_else(|| {
                CassieError::ResourceLimit("alias carrier allocation overflow".into())
            })?;
        };
        spaces.push(Namespace {
            name: name.clone(),
            qualifier: carrier.clone(),
            fields: Vec::new(),
            reserved_identity: false,
            hidden: Vec::new(),
            qualifier_only: true,
        });
        allocated.insert(name, carrier);
    }
    rename_source(&mut select.source, &allocated);
    rename_alias_references(select, &allocated);
    // The fresh carriers no longer shadow original names at this scope; inner
    // scopes still shadow them according to the existing qualified-name laws.
    nested::query(select, &spaces)
}

fn rename_alias_references(select: &mut SelectStatement, aliases: &HashMap<String, String>) {
    let rewrite = |expr: &mut Expr| {
        fn visit(expr: &Expr, aliases: &HashMap<String, String>) -> Expr {
            if let Expr::Column(name) = expr {
                let (qualifier, field) = name
                    .strip_suffix(".*")
                    .map_or((None, None), |qualifier| (Some(qualifier), Some("*")));
                if let Some((qualifier, field)) = qualifier.zip(field) {
                    if let Some(carrier) = aliases.get(qualifier) {
                        return Expr::Column(format!("{carrier}.{field}"));
                    }
                }
                if let Some(carrier) = aliases.get(name) {
                    return Expr::Column(carrier.clone());
                }
                if let Ok(path) = ColumnIdentifierPath::parse(name) {
                    if let Some(qualifier) = path.namespace_qualifier() {
                        if let Some(carrier) = aliases.get(&qualifier) {
                            return Expr::Column(format!("{carrier}.{}", path.field_lookup_key()));
                        }
                    }
                }
            }
            expr.map_children(|child| visit(child, aliases))
        }
        *expr = visit(expr, aliases);
    };
    for item in &mut select.projection {
        match item {
            SelectItem::Column { name, .. } => {
                if let Some(carrier) = aliases.get(name) {
                    name.clone_from(carrier);
                } else if let Ok(path) = ColumnIdentifierPath::parse(name) {
                    if let Some(qualifier) = path.namespace_qualifier() {
                        if let Some(carrier) = aliases.get(&qualifier) {
                            *name = format!("{carrier}.{}", path.field_lookup_key());
                        }
                    }
                }
            }
            SelectItem::Expr { expr, .. } => rewrite(expr),
            SelectItem::Function { function, .. } => {
                for expr in function.expressions_mut() {
                    rewrite(expr);
                }
            }
            SelectItem::WindowFunction { function, .. } => {
                for expr in function.args.iter_mut().chain(&mut function.partition_by) {
                    rewrite(expr);
                }
                for order in &mut function.order_by {
                    rewrite(&mut order.expr);
                }
            }
            SelectItem::Wildcard => {}
        }
    }
    for expr in select
        .filter
        .iter_mut()
        .chain(&mut select.having)
        .chain(&mut select.group_by)
        .chain(&mut select.distinct_on)
        .chain(&mut select.limit)
        .chain(&mut select.offset)
    {
        rewrite(expr);
    }
    for order in &mut select.order {
        rewrite(&mut order.expr);
    }
}

fn scope_names(
    source: &QuerySource,
    aliases: &mut Vec<(String, String)>,
    visible: &mut HashSet<String>,
) {
    match source {
        QuerySource::Aliased { alias, .. } => {
            if let Ok(path) = ColumnIdentifierPath::parse(alias) {
                aliases.push((path.declared_name(), alias.clone()));
            }
        }
        QuerySource::TableSample { source, .. } => scope_names(source, aliases, visible),
        QuerySource::Join { left, right, .. } => {
            scope_names(left, aliases, visible);
            scope_names(right, aliases, visible);
        }
        QuerySource::Collection(name) => visible.extend(crate::catalog::qualifier_variants(name)),
        QuerySource::Cte(name)
        | QuerySource::TableFunction { name, .. }
        | QuerySource::Subquery { alias: name, .. } => {
            visible.extend(crate::catalog::qualifier_variants(name));
        }
        QuerySource::Values { .. } | QuerySource::SingleRow => {}
    }
}

fn rename_source(source: &mut QuerySource, allocated: &HashMap<String, String>) {
    match source {
        QuerySource::TableSample { source, .. } => rename_source(source, allocated),
        QuerySource::Aliased { alias, .. } => {
            if let Ok(path) = ColumnIdentifierPath::parse(alias) {
                if let Some(carrier) = allocated.get(&path.declared_name()) {
                    alias.clone_from(carrier);
                }
            }
        }
        QuerySource::Join { left, right, .. } => {
            rename_source(left, allocated);
            rename_source(right, allocated);
        }
        _ => {}
    }
}

fn query_names(select: &SelectStatement, names: &mut HashSet<String>) {
    source_names(&select.source, names);
    for item in &select.projection {
        match item {
            SelectItem::Column { name, .. } => reference_names(name, names),
            SelectItem::Wildcard => {}
            SelectItem::Expr { expr, .. } => expression_names(expr, names),
            SelectItem::Function { function, .. } => {
                for expr in function.expressions() {
                    expression_names(expr, names);
                }
            }
            SelectItem::WindowFunction { function, .. } => {
                for expr in function.args.iter().chain(&function.partition_by) {
                    expression_names(expr, names);
                }
                for order in &function.order_by {
                    expression_names(&order.expr, names);
                }
            }
        }
    }
    for expr in select
        .filter
        .iter()
        .chain(&select.having)
        .chain(&select.group_by)
        .chain(&select.distinct_on)
        .chain(&select.limit)
        .chain(&select.offset)
    {
        expression_names(expr, names);
    }
    for order in &select.order {
        expression_names(&order.expr, names);
    }
    for cte in &select.ctes {
        match &cte.query {
            CteQuery::Simple(statement) => statement_names(statement, names),
            CteQuery::Recursive {
                base, recursive, ..
            } => {
                statement_names(base, names);
                statement_names(recursive, names);
            }
        }
    }
    if let Some(set) = &select.set {
        query_names(&set.right, names);
    }
}

fn source_names(source: &QuerySource, names: &mut HashSet<String>) {
    match source {
        QuerySource::Aliased { source, alias, .. } => {
            if let Ok(path) = ColumnIdentifierPath::parse(alias) {
                names.insert(path.declared_name());
            }
            names.insert(qualifier(alias));
            source_names(source, names);
        }
        QuerySource::TableSample {
            source,
            rate,
            repeatable,
            ..
        } => {
            source_names(source, names);
            expression_names(rate, names);
            if let Some(seed) = repeatable {
                expression_names(seed, names);
            }
        }
        QuerySource::Join {
            left, right, on, ..
        } => {
            source_names(left, names);
            source_names(right, names);
            expression_names(on, names);
        }
        QuerySource::Collection(name) => names.extend(crate::catalog::qualifier_variants(name)),
        QuerySource::Cte(name) => names.extend(crate::catalog::qualifier_variants(name)),
        QuerySource::Subquery { alias, select, .. } => {
            names.extend(crate::catalog::qualifier_variants(alias));
            query_names(select, names);
        }
        QuerySource::TableFunction { name, function, .. } => {
            names.extend(crate::catalog::qualifier_variants(name));
            for expr in function.expressions() {
                expression_names(expr, names);
            }
        }
        QuerySource::Values { .. } | QuerySource::SingleRow => {}
    }
}

fn statement_names(statement: &crate::sql::ast::ParsedStatement, names: &mut HashSet<String>) {
    if let QueryStatement::Select(select) = &statement.statement {
        query_names(select, names);
    }
}

fn reference_names(name: &str, names: &mut HashSet<String>) {
    if let Ok(path) = ColumnIdentifierPath::parse(name) {
        if let Some(qualifier) = path.namespace_qualifier() {
            names.insert(qualifier);
        }
    }
}

fn expression_names(expr: &Expr, names: &mut HashSet<String>) {
    if let Expr::Column(name) = expr {
        reference_names(name, names);
    }
    if let Some(statement) = expr.subquery_statement() {
        statement_names(statement, names);
    }
    expr.for_each_child(|child| expression_names(child, names));
}
