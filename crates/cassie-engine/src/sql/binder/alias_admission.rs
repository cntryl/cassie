//! Check visible SQL qualifiers before private alias references are generated.
use super::BindingContext;
use super::{
    CassieError, Expr, HashSet, ParsedStatement, QuerySource, QueryStatement, SelectItem,
    SelectStatement,
};
use crate::catalog::Catalog;
use crate::sql::ColumnIdentifierPath;

#[derive(Clone, Default)]
struct Names {
    aliases: HashSet<String>,
    folded: HashSet<String>,
}

pub(super) fn validate(
    statement: &ParsedStatement,
    catalog: &Catalog,
    context: &BindingContext,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<(), CassieError> {
    let result = Walk {
        catalog,
        context,
        controls,
        depth: 0,
    }
    .statement_names(statement, &Names::default(), false);
    let mutation = matches!(
        statement.statement,
        QueryStatement::Insert(_)
            | QueryStatement::Update(_)
            | QueryStatement::Delete(_)
            | QueryStatement::Merge(_)
    );
    result.map_err(|error| match error {
        CassieError::DeferredCorrelatedResolution { name, .. } if mutation => {
            CassieError::UndefinedColumn(name)
        }
        CassieError::DeferredCorrelatedResolution { qualifier, .. } => {
            CassieError::Planner(format!("unknown relation qualifier '{qualifier}'"))
        }
        error => error,
    })
}

#[derive(Clone, Copy)]
struct Walk<'a> {
    catalog: &'a Catalog,
    context: &'a BindingContext,
    controls: Option<&'a crate::runtime::QueryExecutionControls>,
    depth: usize,
}

impl Walk<'_> {
    fn check(&self) -> Result<(), CassieError> {
        if self.depth > 128 {
            return Err(CassieError::ResourceLimit(
                "alias namespace admission exceeds 128 levels".into(),
            ));
        }
        if let Some(controls) = self.controls {
            if controls.is_canceled() {
                return Err(CassieError::QueryCanceled);
            }
            if controls.is_timed_out() {
                return Err(CassieError::DeadlineExceeded);
            }
        }
        Ok(())
    }
    /// Re-parses a stored body with the catalog's type and domain names in scope.
    fn reparse(&self, query: &str) -> Result<ParsedStatement, CassieError> {
        Ok(crate::sql::parse_statement_in_catalog(
            self.catalog,
            self.context,
            query,
        )?)
    }

    fn child(&self) -> Self {
        Self {
            depth: self.depth + 1,
            ..*self
        }
    }

    fn statement_names(
        &self,
        statement: &ParsedStatement,
        outer: &Names,
        active: bool,
    ) -> Result<(), CassieError> {
        self.check()?;
        match &statement.statement {
            QueryStatement::Select(select) => self.query(select, outer, active),
            QueryStatement::Explain(explain) => {
                self.child()
                    .statement_names(&explain.statement, outer, active)
            }
            QueryStatement::Insert(insert) => {
                let mut names = outer.clone();
                mutation_names(
                    &mut names,
                    &insert.table,
                    insert.alias.as_deref(),
                    insert.returning_aliases.as_ref(),
                );
                let outer = &names;
                match &insert.source {
                    super::InsertSource::Select(select) => self.query(select, outer, active)?,
                    super::InsertSource::Values(rows) => {
                        for expr in rows.iter().flatten() {
                            self.expression(expr, outer, active)?;
                        }
                    }
                }
                if let Some(conflict) = &insert.on_conflict {
                    if let crate::sql::ast::InsertConflictAction::DoUpdate {
                        assignments,
                        filter,
                        ..
                    } = &conflict.action
                    {
                        let mut conflict_names = outer.clone();
                        conflict_names.folded.insert("excluded".to_string());
                        let outer = &conflict_names;
                        for (_, expr) in assignments {
                            self.expression(expr, outer, active)?;
                        }
                        if let Some(expr) = filter {
                            self.expression(expr, outer, active)?;
                        }
                    }
                }
                self.items(&insert.returning, outer, active)
            }
            QueryStatement::Update(update) => {
                let mut names = outer.clone();
                mutation_names(
                    &mut names,
                    &update.table,
                    update.alias.as_deref(),
                    update.returning_aliases.as_ref(),
                );
                if let Some(source) = &update.from {
                    self.source_names(&source.source, &mut names)?;
                }
                let outer = &names;
                for (_, expr) in &update.assignments {
                    self.expression(expr, outer, true)?;
                }
                if let Some(expr) = &update.filter {
                    self.expression(expr, outer, true)?;
                }
                self.items(&update.returning, outer, true)
            }
            QueryStatement::Delete(delete) => {
                let mut names = outer.clone();
                mutation_names(
                    &mut names,
                    &delete.table,
                    delete.alias.as_deref(),
                    delete.returning_aliases.as_ref(),
                );
                if let Some(source) = &delete.using {
                    self.source_names(&source.source, &mut names)?;
                }
                let outer = &names;
                if let Some(expr) = &delete.filter {
                    self.expression(expr, outer, true)?;
                }
                self.items(&delete.returning, outer, true)
            }
            QueryStatement::Merge(merge) => self.merge_names(merge, outer),
            QueryStatement::CreateView(view) => {
                let parsed = self.reparse(&view.query)?;
                self.child()
                    .statement_names(&parsed, &Names::default(), false)
            }

            QueryStatement::CreateMaterializedProjection(projection) => {
                let parsed = self.reparse(&projection.query)?;
                self.child()
                    .statement_names(&parsed, &Names::default(), false)
            }
            QueryStatement::CreateMaterializedView(view) => {
                let parsed = self.reparse(&view.query)?;
                self.child()
                    .statement_names(&parsed, &Names::default(), false)
            }
            _ => Ok(()),
        }
    }

    fn merge_names(
        &self,
        merge: &crate::sql::ast::MergeStatement,
        outer: &Names,
    ) -> Result<(), CassieError> {
        let mut names = outer.clone();
        names
            .folded
            .extend(crate::catalog::qualifier_variants(&merge.table));
        names
            .folded
            .extend(merge.alias.as_deref().map(target_alias));
        self.source_names(&merge.source.source, &mut names)?;
        self.expression(&merge.on, &names, true)?;
        for clause in &merge.clauses {
            if let Some(condition) = &clause.condition {
                self.expression(condition, &names, true)?;
            }
            match &clause.action {
                crate::sql::ast::MergeAction::Update {
                    assignments,
                    row_subqueries,
                } => {
                    for (_, expression) in assignments {
                        self.expression(expression, &names, true)?;
                    }
                    for assignment in row_subqueries {
                        self.child()
                            .statement_names(&assignment.query, &names, true)?;
                    }
                }
                crate::sql::ast::MergeAction::Insert {
                    values: Some(values),
                    ..
                } => {
                    for value in values {
                        self.expression(value, &names, true)?;
                    }
                }
                crate::sql::ast::MergeAction::Delete
                | crate::sql::ast::MergeAction::DoNothing
                | crate::sql::ast::MergeAction::Insert { values: None, .. } => {}
            }
        }
        row_image_names(&mut names, merge.returning_aliases.as_ref());
        self.items(&merge.returning, &names, true)
    }

    fn source_names(&self, source: &QuerySource, names: &mut Names) -> Result<bool, CassieError> {
        self.check()?;
        Ok(match source {
            QuerySource::Aliased { alias, .. } => {
                if let Ok(path) = ColumnIdentifierPath::parse(alias) {
                    names.aliases.insert(path.declared_name());
                }
                true
            }
            QuerySource::TableSample { source, .. } => self.child().source_names(source, names)?,
            QuerySource::Join { left, right, .. } => {
                let left = self.child().source_names(left, names)?;
                self.child().source_names(right, names)? || left
            }
            QuerySource::Collection(name) => {
                names
                    .folded
                    .extend(crate::catalog::qualifier_variants(name));
                false
            }
            QuerySource::Cte(name) => {
                names
                    .folded
                    .insert(crate::sql::cte_identity::folded_qualifier(name));
                false
            }
            QuerySource::TableFunction { name, .. } | QuerySource::Subquery { alias: name, .. } => {
                names
                    .folded
                    .extend(crate::catalog::qualifier_variants(name));
                false
            }
            QuerySource::Values { .. } | QuerySource::SingleRow => false,
        })
    }

    fn query(
        &self,
        select: &SelectStatement,
        outer: &Names,
        inherited: bool,
    ) -> Result<(), CassieError> {
        self.check()?;
        let mut visible = outer.clone();
        let active = self.source_names(&select.source, &mut visible)? || inherited;
        self.source(&select.source, &visible, active)?;
        self.items(&select.projection, &visible, active)?;
        for expr in select
            .filter
            .iter()
            .chain(&select.having)
            .chain(&select.group_by)
            .chain(&select.distinct_on)
        {
            self.expression(expr, &visible, active)?;
        }
        // Bounds retain their separately admitted 128-cast envelope.
        let bound_walk = Self { depth: 0, ..*self };
        for expr in select.limit.iter().chain(&select.offset) {
            bound_walk.expression(expr, &visible, active)?;
        }
        for order in &select.order {
            self.expression(&order.expr, &visible, active)?;
        }
        for cte in &select.ctes {
            match &cte.query {
                super::CteQuery::Simple(statement) => {
                    self.child()
                        .statement_names(statement, &Names::default(), false)?;
                }
                super::CteQuery::Recursive {
                    base, recursive, ..
                } => {
                    self.child()
                        .statement_names(base, &Names::default(), false)?;
                    self.child()
                        .statement_names(recursive, &Names::default(), false)?;
                }
            }
        }
        if let Some(set) = &select.set {
            self.child().query(&set.right, outer, inherited)?;
        }
        Ok(())
    }

    fn source(
        &self,
        source: &QuerySource,
        visible: &Names,
        active: bool,
    ) -> Result<(), CassieError> {
        self.check()?;
        match source {
            QuerySource::Aliased { source: inner, .. } => {
                self.child().source(inner, visible, active)
            }
            QuerySource::Join {
                left, right, on, ..
            } => {
                self.child().source(left, visible, active)?;
                self.child().source(right, visible, active)?;
                self.expression(on, visible, active)
            }
            QuerySource::Subquery {
                select, lateral, ..
            } => {
                if *lateral {
                    self.child().query(select, visible, active)
                } else {
                    self.child().query(select, &Names::default(), false)
                }
            }
            QuerySource::TableFunction { function, .. } => {
                for expr in function.expressions() {
                    self.expression(expr, visible, active)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn items(
        &self,
        items: &[SelectItem],
        visible: &Names,
        active: bool,
    ) -> Result<(), CassieError> {
        self.check()?;
        for item in items {
            match item {
                SelectItem::Column { name, .. } => self.column(name, visible, active)?,
                SelectItem::Expr { expr, .. } => self.expression(expr, visible, active)?,
                SelectItem::Function { function, .. } => {
                    for expr in function.expressions() {
                        self.expression(expr, visible, active)?;
                    }
                }
                SelectItem::WindowFunction { function, .. } => {
                    for expr in function
                        .args
                        .iter()
                        .chain(function.filter.as_deref())
                        .chain(&function.partition_by)
                    {
                        self.expression(expr, visible, active)?;
                    }
                    for order in &function.order_by {
                        self.expression(&order.expr, visible, active)?;
                    }
                }
                SelectItem::Wildcard => {}
            }
        }
        Ok(())
    }

    fn expression(&self, expr: &Expr, visible: &Names, active: bool) -> Result<(), CassieError> {
        self.check()?;
        match expr {
            Expr::Column(name) => self.column(name, visible, active),
            Expr::Exists(statement) => self.child().statement_names(statement, visible, active),
            Expr::Subquery(subquery) => {
                for operand in subquery.operands() {
                    self.child().expression(operand, visible, active)?;
                }
                self.child()
                    .statement_names(&subquery.query, visible, active)
            }
            _ => {
                let mut result = Ok(());
                expr.for_each_child(|child| {
                    if result.is_ok() {
                        result = self.child().expression(child, visible, active);
                    }
                });
                result
            }
        }
    }

    fn column(&self, name: &str, visible: &Names, active: bool) -> Result<(), CassieError> {
        self.check()?;
        if !active {
            return Ok(());
        }
        let path = ColumnIdentifierPath::parse(name).map_err(CassieError::Planner)?;
        if !path.is_qualified() {
            return Ok(());
        }
        let key = path.lookup_key();
        let folded = key
            .strip_suffix(&format!(".{}", path.field_lookup_key()))
            .expect("qualified path");
        let qualifier = path.namespace_qualifier();
        if qualifier
            .as_ref()
            .is_some_and(|name| visible.aliases.contains(name))
            || visible.folded.contains(folded)
        {
            Ok(())
        } else {
            let qualifier = qualifier.as_deref().unwrap_or(folded);
            if visible
                .aliases
                .iter()
                .any(|visible| visible.eq_ignore_ascii_case(qualifier))
            {
                return Err(CassieError::UndefinedColumn(name.to_string()));
            }
            Err(CassieError::DeferredCorrelatedResolution {
                name: name.to_string(),
                qualifier: qualifier.to_string(),
            })
        }
    }
}

fn mutation_names(
    names: &mut Names,
    table: &crate::sql::ast::IdentifierPath,
    alias: Option<&str>,
    returning: Option<&crate::sql::ast::ReturningAliases>,
) {
    names
        .folded
        .extend(crate::catalog::qualifier_variants(table));
    names.folded.extend(alias.map(target_alias));
    row_image_names(names, returning);
}

fn row_image_names(names: &mut Names, aliases: Option<&crate::sql::ast::ReturningAliases>) {
    let Some(aliases) = aliases else {
        return;
    };
    names.folded.insert(target_alias(&aliases.old));
    names.folded.insert(target_alias(&aliases.new));
}

/// A mutation target alias is visible to the statement's subqueries.
fn target_alias(alias: &str) -> String {
    ColumnIdentifierPath::parse(alias).map_or_else(
        |_| alias.to_ascii_lowercase(),
        |path| crate::catalog::canonical_identifier_component(&path.declared_name()),
    )
}

#[cfg(test)]
mod cte_identity_tests {
    #[test]
    fn should_keep_a_resolved_cte_in_one_qualification_component() {
        // Arrange
        let catalog = super::Catalog::new();
        let context = super::BindingContext::default();
        let walk = super::Walk {
            catalog: &catalog,
            context: &context,
            controls: None,
            depth: 0,
        };
        let source = super::QuerySource::Cte("part.one".into());
        let mut names = super::Names::default();

        // Act
        walk.source_names(&source, &mut names)
            .expect("source namespace");

        // Assert
        assert!(names.folded.contains("\"part.one\""));
        assert!(!names.folded.contains("one"));
    }
}
