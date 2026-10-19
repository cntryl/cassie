//! Records each sublink's output type and checks its width after binding.
//!
//! Expression typing has no catalog access, so a scalar subquery's type is
//! inferred here, with the enclosing scope's fields visible to a correlated
//! body, and kept on the node. PostgreSQL rejects a sublink of the wrong
//! width while analysing the statement (SQLSTATE 42601), before any row runs.
use super::inference::{
    append_source_qualifiers, infer_cte_schema, infer_select_schema_with_outer,
    infer_source_schema_with_outer,
};
use super::{
    BindingContext, CassieError, Catalog, Expr, HashMap, ParsedStatement, QuerySource,
    QueryStatement, Schema, SelectItem, SelectStatement,
};
use crate::sql::ast::{CteQuery, SubqueryExpr, SubqueryKind};

struct Scope<'a> {
    catalog: &'a Catalog,
    functions: HashMap<String, crate::catalog::FunctionMeta>,
    /// The bound pass checks widths; the earlier pass only supplies types to
    /// the binder's own typing of the enclosing expressions.
    bound: bool,
}

/// Types every sublink before binding, so the binder's own expression typing
/// (such as a VALUES column's common type) sees them. Unresolved names are
/// left for the bound pass.
pub(super) fn annotate_unbound(
    statement: &mut ParsedStatement,
    catalog: &Catalog,
    context: &BindingContext,
) {
    let _ = annotate_with(statement, catalog, context, false);
}

pub(super) fn annotate(
    statement: &mut ParsedStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    annotate_with(statement, catalog, context, true)
}

fn annotate_with(
    statement: &mut ParsedStatement,
    catalog: &Catalog,
    context: &BindingContext,
    bound: bool,
) -> Result<(), CassieError> {
    if !statement_has_sublink(&statement.statement) {
        return Ok(());
    }
    let scope = Scope {
        bound,
        catalog,
        functions: crate::catalog::function_resolution::functions_for_scope(
            &catalog.list_functions(),
            &context.database,
            &context.search_path,
            context.scopes_database_objects(),
        ),
    };
    scope.statement(&mut statement.statement, &HashMap::new(), None)
}

/// Sublinks are rare; a serialized scan avoids walking every clause twice.
fn has_sublink(node: &impl serde::Serialize) -> bool {
    serde_json::to_string(node).is_ok_and(|text| text.contains("{\"Subquery\":{\"kind\""))
}

fn statement_has_sublink(statement: &QueryStatement) -> bool {
    has_sublink(statement)
}

impl Scope<'_> {
    fn statement(
        &self,
        statement: &mut QueryStatement,
        ctes: &HashMap<String, Schema>,
        outer: Option<&Schema>,
    ) -> Result<(), CassieError> {
        match statement {
            QueryStatement::Select(select) => self.select(select, ctes, outer),
            QueryStatement::Explain(explain) => {
                self.statement(&mut explain.statement.statement, ctes, outer)
            }
            QueryStatement::Insert(insert) => {
                let types = self.relation(&insert.table, insert.alias.as_deref());
                if let super::InsertSource::Values(rows) = &mut insert.source {
                    for expr in rows.iter_mut().flatten() {
                        self.expr(expr, ctes, &types)?;
                    }
                }
                if let super::InsertSource::Select(select) = &mut insert.source {
                    self.select(select, ctes, outer)?;
                }
                self.items(&mut insert.returning, ctes, &types)
            }
            QueryStatement::Update(update) => {
                let types = self.relation(&update.table, update.alias.as_deref());
                for (_, expr) in &mut update.assignments {
                    self.expr(expr, ctes, &types)?;
                }
                if let Some(filter) = &mut update.filter {
                    self.expr(filter, ctes, &types)?;
                }
                self.items(&mut update.returning, ctes, &types)
            }
            QueryStatement::Delete(delete) => {
                let types = self.relation(&delete.table, delete.alias.as_deref());
                if let Some(filter) = &mut delete.filter {
                    self.expr(filter, ctes, &types)?;
                }
                self.items(&mut delete.returning, ctes, &types)
            }
            _ => Ok(()),
        }
    }

    fn relation(&self, table: &crate::sql::ast::IdentifierPath, alias: Option<&str>) -> Schema {
        let source = QuerySource::Collection(table.clone());
        let source = match alias {
            Some(alias) => QuerySource::Aliased {
                source: Box::new(source),
                alias: alias.to_string(),
                column_aliases: Vec::new(),
            },
            None => source,
        };
        self.source_scope(&source, &HashMap::new(), None)
    }

    fn source_scope(
        &self,
        source: &QuerySource,
        ctes: &HashMap<String, Schema>,
        outer: Option<&Schema>,
    ) -> Schema {
        let mut schema = infer_source_schema_with_outer(
            source,
            self.catalog,
            ctes,
            &self.functions,
            true,
            &[],
            outer,
        )
        .unwrap_or(Schema { fields: Vec::new() });
        let _ = append_source_qualifiers(source, self.catalog, &mut schema);
        let unqualified = infer_source_schema_with_outer(
            source,
            self.catalog,
            ctes,
            &self.functions,
            false,
            &[],
            outer,
        )
        .unwrap_or(Schema { fields: Vec::new() });
        schema.fields.extend(unqualified.fields);
        if let Some(outer) = outer {
            schema.fields.extend(outer.fields.iter().cloned());
        }
        schema
    }

    fn select(
        &self,
        select: &mut SelectStatement,
        outer_ctes: &HashMap<String, Schema>,
        outer: Option<&Schema>,
    ) -> Result<(), CassieError> {
        if !has_sublink(select) {
            return Ok(());
        }
        let mut ctes = outer_ctes.clone();
        for cte in &mut select.ctes {
            match &mut cte.query {
                CteQuery::Simple(statement) => {
                    self.statement(&mut statement.statement, &ctes, None)?;
                }
                CteQuery::Recursive {
                    base, recursive, ..
                } => {
                    self.statement(&mut base.statement, &ctes, None)?;
                    self.statement(&mut recursive.statement, &ctes, None)?;
                }
            }
            if let Ok(schema) = infer_cte_schema(cte, self.catalog, &ctes, &self.functions) {
                ctes.insert(cte.name.to_ascii_lowercase(), schema);
            }
        }
        self.source(&mut select.source, &ctes, outer)?;
        let scope = self.source_scope(&select.source, &ctes, outer);
        self.items(&mut select.projection, &ctes, &scope)?;
        for expr in select
            .filter
            .iter_mut()
            .chain(select.having.iter_mut())
            .chain(&mut select.group_by)
            .chain(&mut select.distinct_on)
            .chain(select.limit.iter_mut())
            .chain(select.offset.iter_mut())
            .chain(select.order.iter_mut().map(|order| &mut order.expr))
        {
            self.expr(expr, &ctes, &scope)?;
        }
        if let Some(set) = &mut select.set {
            self.select(&mut set.right, &ctes, outer)?;
        }
        Ok(())
    }

    fn source(
        &self,
        source: &mut QuerySource,
        ctes: &HashMap<String, Schema>,
        outer: Option<&Schema>,
    ) -> Result<(), CassieError> {
        match source {
            QuerySource::Aliased { source, .. } => self.source(source, ctes, outer),
            QuerySource::Subquery {
                select, lateral, ..
            } => self.select(select, ctes, if *lateral { outer } else { None }),
            QuerySource::Join {
                left, right, on, ..
            } => {
                self.source(left, ctes, outer)?;
                self.source(right, ctes, outer)?;
                let mut joined = self.source_scope(left, ctes, outer);
                joined
                    .fields
                    .extend(self.source_scope(right, ctes, outer).fields);
                self.expr(on, ctes, &joined)
            }
            QuerySource::TableFunction { function, .. } => {
                let scope = outer.cloned().unwrap_or(Schema { fields: Vec::new() });
                for arg in function.expressions_mut() {
                    self.expr(arg, ctes, &scope)?;
                }
                Ok(())
            }
            QuerySource::Values { rows } => {
                let scope = outer.cloned().unwrap_or(Schema { fields: Vec::new() });
                for expr in rows.iter_mut().flatten() {
                    self.expr(expr, ctes, &scope)?;
                }
                Ok(())
            }
            QuerySource::Collection(_) | QuerySource::Cte(_) | QuerySource::SingleRow => Ok(()),
        }
    }

    fn items(
        &self,
        items: &mut [SelectItem],
        ctes: &HashMap<String, Schema>,
        scope: &Schema,
    ) -> Result<(), CassieError> {
        for item in items {
            match item {
                SelectItem::Expr { expr, .. } => self.expr(expr, ctes, scope)?,
                SelectItem::Function { function, .. } => {
                    for arg in function.expressions_mut() {
                        self.expr(arg, ctes, scope)?;
                    }
                }
                SelectItem::WindowFunction { function, .. } => {
                    for arg in function.args.iter_mut().chain(&mut function.partition_by) {
                        self.expr(arg, ctes, scope)?;
                    }
                    for order in &mut function.order_by {
                        self.expr(&mut order.expr, ctes, scope)?;
                    }
                }
                SelectItem::Wildcard | SelectItem::Column { .. } => {}
            }
        }
        Ok(())
    }

    fn expr(
        &self,
        expr: &mut Expr,
        ctes: &HashMap<String, Schema>,
        scope: &Schema,
    ) -> Result<(), CassieError> {
        match expr {
            Expr::Exists(statement) => {
                return self.statement(&mut statement.statement, ctes, Some(scope));
            }
            Expr::Subquery(subquery) => self.sublink(subquery, ctes, scope)?,
            _ => {}
        }
        expr.try_visit_children_mut(|child| self.expr(child, ctes, scope))
    }

    fn sublink(
        &self,
        subquery: &mut SubqueryExpr,
        ctes: &HashMap<String, Schema>,
        scope: &Schema,
    ) -> Result<(), CassieError> {
        let QueryStatement::Select(select) = &mut subquery.query.statement else {
            return Ok(());
        };
        self.select(select, ctes, Some(scope))?;
        // An uncorrelated body is typed alone; only a correlated one needs
        // the enclosing fields to resolve.
        let inferred = |outer| {
            infer_select_schema_with_outer(select, self.catalog, ctes, &self.functions, &[], outer)
        };
        let Ok(schema) = inferred(None).or_else(|_| inferred(Some(scope))) else {
            return Ok(());
        };
        // A wildcard's width is checked against the executed rows instead.
        let wildcard = select
            .projection
            .iter()
            .any(|item| matches!(item, SelectItem::Wildcard));
        if self.bound && !wildcard && schema.fields.len() != subquery.required_width() {
            return Err(subquery.width_error(schema.fields.len()));
        }
        if matches!(subquery.kind, SubqueryKind::Scalar | SubqueryKind::Array) {
            subquery.data_type = schema.fields.first().map(|field| field.data_type.clone());
        }
        Ok(())
    }
}
