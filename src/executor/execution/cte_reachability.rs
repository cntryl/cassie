//! Scope-aware CTE dependency closure shared by planning and materialization.
use super::{CommonTableExpression, CteQuery, Expr, LogicalPlan, QuerySource, SelectItem};
use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::sql::ast::{OrderExpr, ParsedStatement, QueryStatement, SelectSet, SelectStatement};
use std::ops::ControlFlow;

pub(super) struct SelectParts<'a> {
    pub(super) ctes: &'a [CommonTableExpression],
    pub(super) source: &'a QuerySource,
    pub(super) projection: &'a [SelectItem],
    pub(super) filter: Option<&'a Expr>,
    pub(super) group_by: &'a [Expr],
    pub(super) having: Option<&'a Expr>,
    pub(super) order: &'a [OrderExpr],
    pub(super) distinct_on: &'a [Expr],
    pub(super) limit: Option<&'a Expr>,
    pub(super) offset: Option<&'a Expr>,
    pub(super) set: Option<&'a SelectSet>,
}

impl<'a> SelectParts<'a> {
    pub(super) fn of(select: &'a SelectStatement) -> Self {
        Self {
            ctes: &select.ctes,
            source: &select.source,
            projection: &select.projection,
            filter: select.filter.as_ref(),
            group_by: &select.group_by,
            having: select.having.as_ref(),
            order: &select.order,
            distinct_on: &select.distinct_on,
            limit: select.limit.as_ref(),
            offset: select.offset.as_ref(),
            set: select.set.as_deref(),
        }
    }
}

impl<'a> SelectParts<'a> {
    pub(super) fn plan(plan: &'a LogicalPlan) -> Self {
        Self {
            ctes: &plan.ctes,
            source: &plan.source,
            projection: &plan.projection,
            filter: plan.filter.as_ref(),
            group_by: &plan.group_by,
            having: plan.having.as_ref(),
            order: &plan.order,
            distinct_on: &plan.distinct_on,
            limit: plan.limit.as_ref(),
            offset: plan.offset.as_ref(),
            set: plan.set.as_deref(),
        }
    }
}

const MAX_DEPTH: usize = 128;

pub(crate) struct LiveCtes {
    states: Vec<u8>,
    _memory: QueryMemoryReservation,
}
impl LiveCtes {
    pub(crate) fn contains(&self, index: usize) -> bool {
        self.states[index] != 0
    }
}
struct Scope<'a> {
    ctes: &'a [CommonTableExpression],
    visible: usize,
    live: LiveCtes,
}
struct Walk<'a> {
    scopes: Vec<Scope<'a>>,
    controls: &'a QueryExecutionControls,
}

pub(super) fn used<'a>(
    select: &SelectParts<'a>,
    controls: &'a QueryExecutionControls,
) -> Result<LiveCtes, CassieError> {
    controls.check_interruption()?;
    if select.ctes.is_empty() {
        return Ok(LiveCtes {
            states: Vec::new(),
            _memory: controls.reserve_query_memory(0)?,
        });
    }
    // Bound the scope stack before allocating it; every live mask has its own lease.
    let _stack_memory = controls.reserve_query_memory(size_of::<Scope<'_>>() * MAX_DEPTH)?;
    Walk {
        scopes: Vec::with_capacity(MAX_DEPTH),
        controls,
    }
    .select(select, 0)
}

impl<'a> Walk<'a> {
    fn check(&self, depth: usize) -> Result<(), CassieError> {
        self.controls.check_interruption()?;
        if depth >= MAX_DEPTH {
            return Err(CassieError::ResourceLimit(
                "CTE dependency nesting exceeds 128 levels".into(),
            ));
        }
        Ok(())
    }
    fn select(&mut self, select: &SelectParts<'a>, depth: usize) -> Result<LiveCtes, CassieError> {
        self.check(depth)?;
        let memory = self.controls.reserve_query_memory(select.ctes.len())?;
        self.scopes.push(Scope {
            ctes: select.ctes,
            visible: select.ctes.len(),
            live: LiveCtes {
                states: vec![0; select.ctes.len()],
                _memory: memory,
            },
        });
        self.source(select.source, depth + 1)?;
        for item in select.projection {
            match item {
                SelectItem::Expr { expr, .. } => self.expr(expr, depth + 1)?,
                SelectItem::Function { function, .. } => {
                    for expr in function.expressions() {
                        self.expr(expr, depth + 1)?;
                    }
                }
                SelectItem::WindowFunction { function, .. } => {
                    for expr in function
                        .args
                        .iter()
                        .chain(&function.partition_by)
                        .chain(function.order_by.iter().map(|o| &o.expr))
                    {
                        self.expr(expr, depth + 1)?;
                    }
                }
                SelectItem::Wildcard | SelectItem::Column { .. } => {}
            }
        }
        for expr in select
            .filter
            .into_iter()
            .chain(select.group_by)
            .chain(select.having)
            .chain(select.order.iter().map(|o| &o.expr))
            .chain(select.distinct_on)
            .chain(select.limit)
            .chain(select.offset)
        {
            self.expr(expr, depth + 1)?;
        }
        if let Some(set) = select.set {
            self.select(&SelectParts::of(&set.right), depth + 1)?;
        }
        let scope = self.scopes.len() - 1;
        // Data-changing CTEs must never be discarded because their result is unused.
        for (i, cte) in select.ctes.iter().enumerate() {
            if !matches!(&cte.query, CteQuery::Simple(p) if matches!(p.statement, QueryStatement::Select(_)))
                && !matches!(cte.query, CteQuery::Recursive { .. })
            {
                self.scopes[scope].live.states[i] = 1;
            }
        }
        while let Some(index) = self.scopes[scope]
            .live
            .states
            .iter()
            .position(|state| *state == 1)
        {
            self.check(depth)?;
            self.scopes[scope].live.states[index] = 2;
            let cte = &select.ctes[index];
            self.scopes[scope].visible =
                index + usize::from(matches!(cte.query, CteQuery::Recursive { .. }));
            match &cte.query {
                CteQuery::Simple(statement) => self.statement(statement, depth + 1)?,
                CteQuery::Recursive {
                    base, recursive, ..
                } => {
                    self.statement(base, depth + 1)?;
                    self.statement(recursive, depth + 1)?;
                }
            }
        }
        Ok(self.scopes.pop().expect("scope pushed above").live)
    }
    fn statement(
        &mut self,
        statement: &'a ParsedStatement,
        depth: usize,
    ) -> Result<(), CassieError> {
        self.check(depth)?;
        if let QueryStatement::Select(select) = &statement.statement {
            self.select(&SelectParts::of(select), depth + 1)?;
        }
        Ok(())
    }
    fn relation(&mut self, name: &str) {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(index) = scope.ctes[..scope.visible]
                .iter()
                .position(|cte| cte.name.eq_ignore_ascii_case(name))
            {
                if scope.live.states[index] == 0 {
                    scope.live.states[index] = 1;
                }
                return;
            }
        }
    }
    fn source(&mut self, source: &'a QuerySource, depth: usize) -> Result<(), CassieError> {
        self.check(depth)?;
        match source {
            QuerySource::Cte(name) => self.relation(name),
            QuerySource::Collection(path) => {
                let mut names = path.values();
                if let Some(name) = names.next() {
                    if names.next().is_none() {
                        self.relation(name);
                    }
                }
            }
            QuerySource::Aliased { source, .. } => self.source(source, depth + 1)?,
            QuerySource::Subquery { select, .. } => {
                self.select(&SelectParts::of(select), depth + 1)?;
            }
            QuerySource::Join {
                left, right, on, ..
            } => {
                self.source(left, depth + 1)?;
                self.source(right, depth + 1)?;
                self.expr(on, depth + 1)?;
            }
            QuerySource::Values { rows } => {
                for expr in rows.iter().flatten() {
                    self.expr(expr, depth + 1)?;
                }
            }
            QuerySource::TableFunction { function, .. } => {
                for expr in function.expressions() {
                    self.expr(expr, depth + 1)?;
                }
            }
            QuerySource::SingleRow => {}
        }
        Ok(())
    }
    fn expr(&mut self, expr: &'a Expr, depth: usize) -> Result<(), CassieError> {
        self.check(depth)?;
        if let Expr::Exists(statement) = expr {
            return self.statement(statement, depth + 1);
        }
        if let Expr::Subquery(subquery) = expr {
            self.statement(&subquery.query, depth + 1)?;
        }
        match expr.try_for_each_child(|child| match self.expr(child, depth + 1) {
            Ok(()) => ControlFlow::Continue(()),
            Err(e) => ControlFlow::Break(e),
        }) {
            ControlFlow::Continue(()) => Ok(()),
            ControlFlow::Break(error) => Err(error),
        }
    }
}

/// Share the same reached-definition mask with pre-execution bound resolution.
pub(crate) fn for_plan(
    plan: &LogicalPlan,
    controls: &QueryExecutionControls,
) -> Result<LiveCtes, CassieError> {
    used(&SelectParts::plan(plan), controls)
}

pub(crate) fn for_select(
    select: &SelectStatement,
    controls: &QueryExecutionControls,
) -> Result<LiveCtes, CassieError> {
    used(&SelectParts::of(select), controls)
}
