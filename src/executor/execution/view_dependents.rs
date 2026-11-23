//! Views that depend on a relation or a column, found from the parsed view
//! definition rather than from its SQL text.
//!
//! A view stores its definition as SQL text. Dependencies are read from the
//! parsed `SELECT`: relations by the sources of every scope (joins of every
//! spelling, subqueries, CTE bodies), columns by the references that resolve to
//! a source of the target table. `*` is expanded at creation in PostgreSQL, so
//! a wildcard depends on every column the view exposed.

use std::collections::BTreeSet;

use super::domain_view_dependents::parse_view;
use super::{Cassie, QueryError};
use crate::catalog::{ParsedName, ViewMeta};
use crate::sql::ast::{
    CteQuery, Expr, ParsedStatement, QuerySource, QueryStatement, SelectItem, SelectStatement,
};
use crate::sql::ColumnIdentifierPath;

pub(super) fn parsed_select(cassie: &Cassie, view: &ViewMeta) -> Option<SelectStatement> {
    match parse_view(cassie, &view.query, None)?.statement {
        QueryStatement::Select(select) => Some(*select),
        _ => None,
    }
}

/// The stored name of the table or view `written` denotes, when it exists.
fn canonical_relation(cassie: &Cassie, written: &str) -> Option<String> {
    cassie
        .catalog
        .get_collection(written)
        .map(|collection| collection.name)
        .or_else(|| cassie.catalog.get_view(written).map(|view| view.name))
}

fn same_relation(cassie: &Cassie, written: &str, target: &str) -> bool {
    canonical_relation(cassie, written)
        .is_some_and(|stored| canonical_relation(cassie, target).is_none_or(|t| t == stored))
}

/// Views whose definition reads `relation` directly.
pub(super) fn views_reading(cassie: &Cassie, relation: &str) -> Vec<ViewMeta> {
    cassie
        .catalog
        .list_views()
        .into_iter()
        .filter(|view| {
            parsed_select(cassie, view).is_some_and(|select| {
                crate::sql::binder::select_referenced_relations(&select)
                    .iter()
                    .any(|written| same_relation(cassie, written, relation))
            })
        })
        .collect()
}

/// Every view that reads one of `seeds`, transitively, including the seeds.
pub(super) fn with_transitive_dependents(cassie: &Cassie, seeds: Vec<ViewMeta>) -> Vec<ViewMeta> {
    let mut all = seeds;
    let mut next = 0;
    while next < all.len() {
        let name = all[next].name.clone();
        next += 1;
        for view in views_reading(cassie, &name) {
            if all.iter().all(|known| known.name != view.name) {
                all.push(view);
            }
        }
    }
    all
}

/// Views whose parsed definition resolves a call to `function`.
pub(super) fn views_calling_function(
    cassie: &Cassie,
    function: &crate::catalog::FunctionMeta,
) -> Vec<ViewMeta> {
    let Ok(ParsedName::DatabaseQualified {
        database,
        schema,
        name,
    }) = crate::catalog::parse_name(&function.name)
    else {
        return Vec::new();
    };
    cassie
        .catalog
        .list_views()
        .into_iter()
        .filter(|view| {
            if !view.function_dependency_database.is_empty() {
                return view
                    .function_dependency_ids
                    .iter()
                    .any(|object_id| *object_id == function.object_id);
            }
            parsed_select(cassie, view).is_some_and(|select| {
                let mut calls = Vec::new();
                crate::sql::binder::collect_functions(&select, &mut calls);
                calls.into_iter().any(|call| {
                    call_resolves_legacy_function(
                        &call.name,
                        (&database, &schema, &name),
                        crate::catalog::relation_database_name(&view.name)
                            .unwrap_or_else(|| cassie.default_database.clone()),
                        &database,
                    )
                })
            })
        })
        .collect()
}

fn call_resolves_legacy_function(
    written: &str,
    target: (&str, &str, &str),
    view_database: String,
    function_database: &str,
) -> bool {
    if !view_database.eq_ignore_ascii_case(function_database) {
        return false;
    }
    let (database, schema, name) = target;
    match crate::catalog::parse_name(written) {
        Ok(ParsedName::Unqualified(written)) => written.eq_ignore_ascii_case(name),
        Ok(ParsedName::SchemaQualified {
            schema: written_schema,
            name: written,
        }) => written.eq_ignore_ascii_case(name) && written_schema.eq_ignore_ascii_case(schema),
        Ok(ParsedName::DatabaseQualified {
            database: written_database,
            schema: written_schema,
            name: written,
        }) => {
            written.eq_ignore_ascii_case(name)
                && written_schema.eq_ignore_ascii_case(schema)
                && written_database.eq_ignore_ascii_case(database)
        }
        Err(_) => false,
    }
}

pub(super) fn reject_function_view_dependents(
    cassie: &Cassie,
    function: &crate::catalog::FunctionMeta,
) -> Result<(), QueryError> {
    let direct = views_calling_function(cassie, function);
    let views = with_transitive_dependents(cassie, direct);
    let Some(dependent) = views.first() else {
        return Ok(());
    };
    let display_name = crate::catalog::local_name(&function.name);
    Err(QueryError::Cassie(
        crate::app::DiagnosticError::new(
            "2BP01",
            format!("cannot drop function {display_name} because other objects depend on it"),
        )
        .with_detail(format!(
            "view {} depends on function {display_name}",
            dependent.name
        ))
        .with_hint("Use DROP FUNCTION ... CASCADE to drop the dependent objects too")
        .into(),
    ))
}

/// Views whose definition uses `column` of `table`.
pub(super) fn views_using_column(cassie: &Cassie, table: &str, column: &str) -> Vec<ViewMeta> {
    let Some(target) = canonical_relation(cassie, table) else {
        return Vec::new();
    };
    cassie
        .catalog
        .list_views()
        .into_iter()
        .filter(|view| {
            parsed_select(cassie, view).is_some_and(|select| {
                let mut walker = Walker {
                    cassie,
                    target: &target,
                    column,
                    view,
                    scopes: Vec::new(),
                    depth: 0,
                    ctes: Vec::new(),
                    found: false,
                };
                walker.select(&select);
                walker.found
            })
        })
        .collect()
}

struct Source {
    names: Vec<String>,
    relation: Option<String>,
}

struct Walker<'a> {
    cassie: &'a Cassie,
    target: &'a str,
    column: &'a str,
    view: &'a ViewMeta,
    scopes: Vec<Vec<Source>>,
    /// Nesting depth below the view's own select (zero at the top level).
    depth: usize,
    ctes: Vec<String>,
    found: bool,
}

fn fold(name: &str) -> String {
    name.trim_matches('"').to_ascii_lowercase()
}

impl Walker<'_> {
    fn columns_of(&self, relation: &str) -> Vec<String> {
        if self.cassie.catalog.get_collection(relation).is_some() {
            return self.cassie.midge.row_schema(relation).map_or_else(
                |_| Vec::new(),
                |schema| {
                    schema
                        .fields
                        .iter()
                        .filter(|field| !field.retired)
                        .map(|field| field.name.clone())
                        .collect()
                },
            );
        }
        self.cassie
            .catalog
            .get_view(relation)
            .map_or_else(Vec::new, |view| {
                view.schema.fields.into_iter().map(|f| f.name).collect()
            })
    }

    fn has_column(&self, relation: &str, column: &str) -> bool {
        self.columns_of(relation)
            .iter()
            .any(|stored| ColumnIdentifierPath::matches_stored_field(column, stored))
    }

    fn target_has_column(&self) -> bool {
        self.has_column(self.target, self.column)
    }

    fn statement(&mut self, statement: &ParsedStatement) {
        if let QueryStatement::Select(select) = &statement.statement {
            self.nested_select(select);
        }
    }

    fn nested_select(&mut self, select: &SelectStatement) {
        self.depth += 1;
        self.select(select);
        self.depth -= 1;
    }

    fn select(&mut self, select: &SelectStatement) {
        let inherited = self.ctes.len();
        for cte in &select.ctes {
            match &cte.query {
                CteQuery::Simple(statement) => self.statement(statement),
                CteQuery::Recursive {
                    base, recursive, ..
                } => {
                    self.statement(base);
                    self.ctes.push(cte.name.clone());
                    self.statement(recursive);
                }
            }
            self.ctes.push(cte.name.clone());
        }
        let mut sources = Vec::new();
        let mut conditions = Vec::new();
        self.sources(&select.source, &mut sources, &mut conditions);
        self.scopes.push(sources);

        let aliases: BTreeSet<String> = select
            .projection
            .iter()
            .filter_map(|item| match item {
                SelectItem::Column { alias, .. }
                | SelectItem::Function { alias, .. }
                | SelectItem::Expr { alias, .. }
                | SelectItem::WindowFunction { alias, .. } => alias.as_deref().map(fold),
                SelectItem::Wildcard => None,
            })
            .collect();
        for item in &select.projection {
            self.item(item);
        }
        for expression in conditions
            .into_iter()
            .chain(select.filter.iter())
            .chain(&select.distinct_on)
            .chain(&select.group_by)
            .chain(select.having.iter())
            .chain(select.limit.iter())
            .chain(select.offset.iter())
        {
            self.expression(expression);
        }
        for order in &select.order {
            // An output alias wins over a column of the same name in ORDER BY.
            if matches!(&order.expr, Expr::Column(name) if aliases.contains(&fold(name))) {
                continue;
            }
            self.expression(&order.expr);
        }
        self.scopes.pop();
        self.ctes.truncate(inherited);
        if let Some(set) = &select.set {
            self.select(&set.right);
        }
    }

    fn sources<'s>(
        &mut self,
        source: &'s QuerySource,
        out: &mut Vec<Source>,
        conditions: &mut Vec<&'s Expr>,
    ) {
        match source {
            QuerySource::Collection(path) => {
                let written = path.to_string();
                let local = path
                    .components()
                    .last()
                    .map_or_else(String::new, |component| fold(component.value()));
                let is_cte = crate::sql::cte_identity::reference_name(path)
                    .is_some_and(|name| self.ctes.iter().any(|cte| cte == name));
                out.push(Source {
                    names: vec![local, fold(&written)],
                    relation: (!is_cte)
                        .then(|| canonical_relation(self.cassie, &written))
                        .flatten(),
                });
            }
            QuerySource::Aliased { source, alias, .. } => {
                let mut inner = Vec::new();
                self.sources(source, &mut inner, conditions);
                for mut found in inner {
                    found.names = vec![fold(alias)];
                    out.push(found);
                }
            }
            QuerySource::TableSample { source, .. } => self.sources(source, out, conditions),
            QuerySource::Join {
                left, right, on, ..
            } => {
                self.sources(left, out, conditions);
                self.sources(right, out, conditions);
                conditions.push(on);
            }
            QuerySource::Subquery { alias, select, .. } => {
                self.nested_select(select);
                out.push(Source {
                    names: vec![fold(alias)],
                    relation: None,
                });
            }
            QuerySource::TableFunction { name, function, .. } => {
                for expression in function.expressions() {
                    self.expression(expression);
                }
                out.push(Source {
                    names: vec![fold(name)],
                    relation: None,
                });
            }
            QuerySource::Values { .. } | QuerySource::Cte(_) | QuerySource::SingleRow => {}
        }
    }

    fn item(&mut self, item: &SelectItem) {
        match item {
            SelectItem::Wildcard => self.wildcard(None),
            SelectItem::Column { name, .. } => self.column(name),
            SelectItem::Expr { expr, .. } => self.expression(expr),
            SelectItem::Function { function, .. } => {
                for expression in function.expressions() {
                    self.expression(expression);
                }
            }
            SelectItem::WindowFunction { function, .. } => {
                for expression in function
                    .args
                    .iter()
                    .chain(function.filter.as_deref())
                    .chain(&function.partition_by)
                    .chain(function.order_by.iter().map(|order| &order.expr))
                {
                    self.expression(expression);
                }
            }
        }
    }

    fn expression(&mut self, expression: &Expr) {
        expression.any_descendant_or_self(&mut |candidate| {
            match candidate {
                Expr::Column(name) => self.column(name),
                Expr::Exists(statement) => self.statement(statement),
                Expr::Subquery(subquery) => self.statement(&subquery.query),
                _ => {}
            }
            false
        });
    }

    fn scope_index_of(&self, qualifiers: &[String]) -> Option<(usize, usize)> {
        self.scopes
            .iter()
            .enumerate()
            .rev()
            .find_map(|(scope, sources)| {
                sources
                    .iter()
                    .position(|source| {
                        source
                            .names
                            .iter()
                            .any(|name| qualifiers.iter().any(|qualifier| qualifier == name))
                    })
                    .map(|index| (scope, index))
            })
    }

    fn is_target(&self, scope: usize, index: usize) -> bool {
        self.scopes[scope][index].relation.as_deref() == Some(self.target)
    }

    /// A wildcard, optionally qualified, depends on each column the view
    /// exposed at creation, which the stored view schema records.
    fn wildcard(&mut self, qualifiers: Option<&[String]>) {
        let Some(scope) = self.scopes.len().checked_sub(1) else {
            return;
        };
        let reads_target = match qualifiers {
            Some(qualifiers) => self
                .scope_index_of(qualifiers)
                .is_some_and(|(found, index)| self.is_target(found, index)),
            None => (0..self.scopes[scope].len()).any(|index| self.is_target(scope, index)),
        };
        if !reads_target || !self.target_has_column() {
            return;
        }
        let exposed = self
            .view
            .schema
            .fields
            .iter()
            .any(|field| ColumnIdentifierPath::matches_stored_field(self.column, &field.name));
        if exposed || self.depth > 0 {
            self.found = true;
        }
    }

    fn column(&mut self, raw: &str) {
        if self.found {
            return;
        }
        let Ok(path) = ColumnIdentifierPath::parse(raw) else {
            return;
        };
        let qualified = path.is_qualified();
        let owner_name = path.namespace_prefix_keys().last().map(|key| fold(key));
        if path.final_name() == "*" {
            if let Some(owner_name) = owner_name {
                self.wildcard(Some(&[owner_name]));
            }
            return;
        }
        if path.field_lookup_key() != ColumnIdentifierPath::stored_field_key(self.column) {
            return;
        }
        if qualified {
            let Some(owner_name) = owner_name else {
                return;
            };
            if let Some((scope, index)) = self.scope_index_of(&[owner_name]) {
                self.found = self.is_target(scope, index);
            }
            return;
        }
        // An unqualified name belongs to the innermost scope that has a source
        // with that column.
        for scope in (0..self.scopes.len()).rev() {
            let owner = self.scopes[scope].iter().position(|source| {
                source
                    .relation
                    .as_deref()
                    .is_some_and(|relation| self.has_column(relation, self.column))
            });
            if let Some(index) = owner {
                self.found = self.is_target(scope, index);
                return;
            }
        }
    }
}

/// Views store their definition as SQL text and Cassie has no renderer that
/// could rewrite it, so a rename that a view depends on is refused rather than
/// leaving the view unresolvable (PostgreSQL rewrites the view instead).
pub(super) fn reject_view_dependents(
    cassie: &Cassie,
    action: &str,
    kind: &str,
    name: &str,
    views: Vec<ViewMeta>,
) -> Result<(), QueryError> {
    let views = with_transitive_dependents(cassie, views);
    if views.is_empty() {
        return Ok(());
    }
    let display_name = name.rsplit('.').next().unwrap_or(name);
    let dependent = views
        .iter()
        .map(|view| format!("view {}", view.name))
        .collect::<Vec<_>>()
        .join(", ");
    Err(QueryError::Cassie(
        crate::app::DiagnosticError::new(
            "2BP01",
            format!("cannot {action} {kind} {display_name} because other objects depend on it"),
        )
        .with_detail(format!("dependent objects: {dependent}"))
        .into(),
    ))
}

/// A view records the output types of its columns, so a type change under a
/// view is refused as PostgreSQL does.
pub(super) fn reject_column_type_under_views(
    cassie: &Cassie,
    table: &str,
    field: &str,
) -> Result<(), QueryError> {
    let views = views_using_column(cassie, table, field);
    if views.is_empty() {
        return Ok(());
    }
    Err(QueryError::Cassie(
        crate::app::DiagnosticError::new(
            "0A000",
            "cannot alter type of a column used by a view or rule",
        )
        .with_detail(format!("view {} depends on column {field}", views[0].name))
        .into(),
    ))
}
