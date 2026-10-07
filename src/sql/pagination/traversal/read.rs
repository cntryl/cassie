//! Read-only admission before any temporary plan counting or cloning.

use crate::app::CassieError;
use crate::planner::logical::LogicalPlan;
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{
    CteQuery, Expr, ParsedStatement, QuerySource, QueryStatement, SelectItem, SelectStatement,
};

pub(in crate::sql::pagination) fn plan_needs_resolution(
    plan: &LogicalPlan,
    controls: &QueryExecutionControls,
) -> Result<bool, CassieError> {
    let mut reader = Reader {
        controls,
        needed: false,
    };
    reader.guard(0)?;
    reader.bounds(plan.limit.as_ref(), plan.offset.as_ref())?;
    reader.source(&plan.source, 1)?;
    for cte in &plan.ctes {
        reader.cte(cte, 1)?;
    }
    if let Some(set) = &plan.set {
        reader.select(&set.right, 1)?;
    }
    for item in &plan.projection {
        reader.item(item, 1)?;
    }
    for expr in plan
        .filter
        .iter()
        .chain(&plan.having)
        .chain(&plan.distinct_on)
        .chain(&plan.group_by)
    {
        reader.expr(expr, 1)?;
    }
    for order in &plan.order {
        reader.expr(&order.expr, 1)?;
    }
    Ok(reader.needed)
}

struct Reader<'a> {
    controls: &'a QueryExecutionControls,
    needed: bool,
}

impl Reader<'_> {
    fn guard(&self, depth: usize) -> Result<(), CassieError> {
        if self.controls.is_cancelled() {
            return Err(CassieError::QueryCancelled);
        }
        if self.controls.is_timed_out() {
            return Err(CassieError::DeadlineExceeded);
        }
        if depth >= 128 {
            return Err(CassieError::ResourceLimit(
                "pagination plan nesting exceeds 128 traversal levels".into(),
            ));
        }
        Ok(())
    }

    fn bounds(&mut self, limit: Option<&Expr>, offset: Option<&Expr>) -> Result<(), CassieError> {
        // Bound expressions retain their own admitted 128-cast envelope rather
        // than adding their ancestors to the structural traversal depth.
        self.guard(0)?;
        for expr in limit.into_iter().chain(offset) {
            self.bound_depth(expr, 0)?;
            if !super::super::is_admitted(expr) {
                return Err(super::super::type_error());
            }
        }
        self.needed |= super::super::bound_needs_resolution(limit)
            | super::super::bound_needs_resolution(offset);
        Ok(())
    }

    fn bound_depth(&self, expr: &Expr, depth: usize) -> Result<(), CassieError> {
        self.guard(0)?;
        if depth > 128 {
            return Err(CassieError::ResourceLimit(
                "pagination plan nesting exceeds 128 bound levels".into(),
            ));
        }
        match expr {
            Expr::Cast { expr, .. } => self.bound_depth(expr, depth + 1)?,
            Expr::Binary { left, right, .. } => {
                self.bound_depth(left, depth + 1)?;
                self.bound_depth(right, depth + 1)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn source(&mut self, source: &QuerySource, depth: usize) -> Result<(), CassieError> {
        self.guard(depth)?;
        match source {
            QuerySource::Aliased { source, .. } => self.source(source, depth + 1)?,
            QuerySource::Subquery { select, .. } => self.select(select, depth + 1)?,
            QuerySource::Join {
                left, right, on, ..
            } => {
                self.source(left, depth + 1)?;
                self.source(right, depth + 1)?;
                self.expr(on, depth + 1)?;
            }
            QuerySource::TableFunction { function, .. } => {
                for expr in &function.args {
                    self.expr(expr, depth + 1)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn cte(
        &mut self,
        cte: &crate::sql::ast::CommonTableExpression,
        depth: usize,
    ) -> Result<(), CassieError> {
        self.guard(depth)?;
        match &cte.query {
            CteQuery::Simple(statement) => self.statement(statement, depth + 1)?,
            CteQuery::Recursive {
                base, recursive, ..
            } => {
                self.statement(base, depth + 1)?;
                self.statement(recursive, depth + 1)?;
            }
        }
        Ok(())
    }

    fn statement(&mut self, statement: &ParsedStatement, depth: usize) -> Result<(), CassieError> {
        self.guard(depth)?;
        match &statement.statement {
            QueryStatement::Select(select) => self.select(select, depth + 1)?,
            QueryStatement::Explain(explain) => self.statement(&explain.statement, depth + 1)?,
            _ => {}
        }
        Ok(())
    }

    fn select(&mut self, select: &SelectStatement, depth: usize) -> Result<(), CassieError> {
        self.guard(depth)?;
        self.bounds(select.limit.as_ref(), select.offset.as_ref())?;
        self.source(&select.source, depth + 1)?;
        for cte in &select.ctes {
            self.cte(cte, depth + 1)?;
        }
        if let Some(set) = &select.set {
            self.select(&set.right, depth + 1)?;
        }
        for item in &select.projection {
            self.item(item, depth + 1)?;
        }
        for expr in select
            .filter
            .iter()
            .chain(&select.having)
            .chain(&select.distinct_on)
            .chain(&select.group_by)
        {
            self.expr(expr, depth + 1)?;
        }
        for order in &select.order {
            self.expr(&order.expr, depth + 1)?;
        }
        Ok(())
    }

    fn item(&mut self, item: &SelectItem, depth: usize) -> Result<(), CassieError> {
        self.guard(depth)?;
        match item {
            SelectItem::Expr { expr, .. } => self.expr(expr, depth + 1)?,
            SelectItem::Function { function, .. } => {
                for expr in &function.args {
                    self.expr(expr, depth + 1)?;
                }
            }
            SelectItem::WindowFunction { function, .. } => {
                for expr in function.args.iter().chain(&function.partition_by) {
                    self.expr(expr, depth + 1)?;
                }
                for order in &function.order_by {
                    self.expr(&order.expr, depth + 1)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn expr(&mut self, expr: &Expr, depth: usize) -> Result<(), CassieError> {
        self.guard(depth)?;
        match expr {
            Expr::Exists(statement) => self.statement(statement, depth + 1)?,
            Expr::Binary { left, right, .. } => {
                self.expr(left, depth + 1)?;
                self.expr(right, depth + 1)?;
            }
            Expr::Cast { expr, .. } | Expr::IsNull { expr, .. } | Expr::Not { expr } => {
                self.expr(expr, depth + 1)?
            }
            Expr::InList { expr, values, .. } => {
                self.expr(expr, depth + 1)?;
                for value in values {
                    self.expr(value, depth + 1)?;
                }
            }
            Expr::Between {
                expr, low, high, ..
            } => {
                self.expr(expr, depth + 1)?;
                self.expr(low, depth + 1)?;
                self.expr(high, depth + 1)?;
            }
            Expr::Case {
                operand,
                branches,
                else_expr,
            } => {
                if let Some(expr) = operand {
                    self.expr(expr, depth + 1)?;
                }
                for (when, then) in branches {
                    self.expr(when, depth + 1)?;
                    self.expr(then, depth + 1)?;
                }
                if let Some(expr) = else_expr {
                    self.expr(expr, depth + 1)?;
                }
            }
            Expr::Function(function) => {
                for expr in &function.args {
                    self.expr(expr, depth + 1)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}
