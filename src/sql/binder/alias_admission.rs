//! Check visible SQL qualifiers before private alias references are generated.
use super::{
    CassieError, Expr, HashSet, ParsedStatement, QuerySource, QueryStatement, SelectItem,
    SelectStatement,
};
use crate::sql::ColumnIdentifierPath;

#[derive(Clone, Default)]
struct Names {
    aliases: HashSet<String>,
    folded: HashSet<String>,
}

pub(super) fn validate(
    statement: &ParsedStatement,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<(), CassieError> {
    Walk { controls, depth: 0 }.statement_names(statement, &Names::default(), false)
}

#[derive(Clone, Copy)]
struct Walk<'a> {
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
            if controls.is_cancelled() {
                return Err(CassieError::QueryCancelled);
            }
            if controls.is_timed_out() {
                return Err(CassieError::DeadlineExceeded);
            }
        }
        Ok(())
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
                names
                    .folded
                    .extend(crate::catalog::qualifier_variants(&insert.table));
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
                names
                    .folded
                    .extend(crate::catalog::qualifier_variants(&update.table));
                let outer = &names;
                for (_, expr) in &update.assignments {
                    self.expression(expr, outer, active)?;
                }
                if let Some(expr) = &update.filter {
                    self.expression(expr, outer, active)?;
                }
                self.items(&update.returning, outer, active)
            }
            QueryStatement::Delete(delete) => {
                let mut names = outer.clone();
                names
                    .folded
                    .extend(crate::catalog::qualifier_variants(&delete.table));
                let outer = &names;
                if let Some(expr) = &delete.filter {
                    self.expression(expr, outer, active)?;
                }
                self.items(&delete.returning, outer, active)
            }
            QueryStatement::CreateView(view) => {
                let parsed = crate::sql::parser::parse_statement(&view.query)?;
                self.child()
                    .statement_names(&parsed, &Names::default(), false)
            }
            QueryStatement::CreateMaterializedProjection(projection) => {
                let parsed = crate::sql::parser::parse_statement(&projection.query)?;
                self.child()
                    .statement_names(&parsed, &Names::default(), false)
            }
            _ => Ok(()),
        }
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
            QuerySource::Cte(name)
            | QuerySource::TableFunction { name, .. }
            | QuerySource::Subquery { alias: name, .. } => {
                names
                    .folded
                    .extend(crate::catalog::qualifier_variants(name));
                false
            }
            QuerySource::SingleRow => false,
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
                for expr in &function.args {
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
                    for expr in &function.args {
                        self.expression(expr, visible, active)?;
                    }
                }
                SelectItem::WindowFunction { function, .. } => {
                    for expr in function.args.iter().chain(&function.partition_by) {
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
        let qualifier = path.namespace_qualifier().unwrap_or_else(|| {
            path.lookup_key()
                .strip_suffix(&format!(".{}", path.field_lookup_key()))
                .expect("qualified path")
                .to_string()
        });
        if visible.aliases.contains(&qualifier)
            || visible.folded.contains(&qualifier.to_ascii_lowercase())
        {
            Ok(())
        } else {
            Err(CassieError::Planner(format!(
                "unresolvable column reference '{name}': unknown relation qualifier '{qualifier}'"
            )))
        }
    }
}
