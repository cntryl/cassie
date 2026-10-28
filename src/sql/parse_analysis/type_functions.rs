//! Preserve native UNKNOWN parameter coercion before effective input OIDs erase its origin.
use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::sql::ast::{
    CastIdentity, CteQuery, Expr, FunctionCall, InsertSource, ParsedStatement, QuerySource,
    QueryStatement, SelectItem, SelectStatement,
};
use crate::types::DataType;

pub(crate) fn normalize(
    parsed: &mut ParsedStatement,
    declared: &[i32],
    controls: &QueryExecutionControls,
) -> Result<(), CassieError> {
    Normalizer { declared, controls }.statement(parsed, 0)
}

struct Normalizer<'a> {
    declared: &'a [i32],
    controls: &'a QueryExecutionControls,
}

struct Coercion {
    expression: Expr,
    label: &'static str,
    _scratch: QueryMemoryReservation,
}

impl Normalizer<'_> {
    fn check(&self, depth: usize) -> Result<(), CassieError> {
        self.controls.check_interruption()?;
        if depth > 128 {
            return Err(CassieError::ResourceLimit(
                "type-function analysis nesting exceeds 128 levels".into(),
            ));
        }
        Ok(())
    }

    fn statement(&self, parsed: &mut ParsedStatement, depth: usize) -> Result<(), CassieError> {
        self.check(depth)?;
        match &mut parsed.statement {
            QueryStatement::Select(select) => self.select(select, depth + 1)?,
            QueryStatement::Explain(explain) => {
                self.statement(&mut explain.statement, depth + 1)?;
            }
            QueryStatement::Update(update) => {
                for (_, expr) in &mut update.assignments {
                    self.expression(expr, depth + 1)?;
                }
                if let Some(expr) = &mut update.filter {
                    self.expression(expr, depth + 1)?;
                }
                self.items(&mut update.returning, depth + 1)?;
            }
            QueryStatement::CallProcedure(procedure) => {
                for expr in &mut procedure.args {
                    self.expression(expr, depth + 1)?;
                }
            }
            QueryStatement::Delete(delete) => {
                if let Some(expr) = &mut delete.filter {
                    self.expression(expr, depth + 1)?;
                }
                self.items(&mut delete.returning, depth + 1)?;
            }
            QueryStatement::Insert(insert) => {
                match &mut insert.source {
                    InsertSource::Values(rows) => {
                        for expr in rows.iter_mut().flatten() {
                            self.expression(expr, depth + 1)?;
                        }
                    }
                    InsertSource::Select(select) => self.select(select, depth + 1)?,
                }
                if let Some(conflict) = &mut insert.on_conflict {
                    if let crate::sql::ast::InsertConflictAction::DoUpdate {
                        assignments,
                        filter,
                        ..
                    } = &mut conflict.action
                    {
                        for (_, expr) in assignments {
                            self.expression(expr, depth + 1)?;
                        }
                        if let Some(expr) = filter {
                            self.expression(expr, depth + 1)?;
                        }
                    }
                }
                self.items(&mut insert.returning, depth + 1)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn select(&self, select: &mut SelectStatement, depth: usize) -> Result<(), CassieError> {
        self.check(depth)?;
        self.source(&mut select.source, depth + 1)?;
        self.items(&mut select.projection, depth + 1)?;
        for expr in select
            .filter
            .iter_mut()
            .chain(&mut select.having)
            .chain(&mut select.group_by)
            .chain(&mut select.distinct_on)
            .chain(select.order.iter_mut().map(|order| &mut order.expr))
            .chain(&mut select.limit)
            .chain(&mut select.offset)
        {
            self.expression(expr, depth + 1)?;
        }
        for cte in &mut select.ctes {
            match &mut cte.query {
                CteQuery::Simple(statement) => self.statement(statement, depth + 1)?,
                CteQuery::Recursive {
                    base, recursive, ..
                } => {
                    self.statement(base, depth + 1)?;
                    self.statement(recursive, depth + 1)?;
                }
            }
        }
        if let Some(set) = &mut select.set {
            self.select(&mut set.right, depth + 1)?;
        }
        Ok(())
    }

    fn source(&self, source: &mut QuerySource, depth: usize) -> Result<(), CassieError> {
        self.check(depth)?;
        match source {
            QuerySource::Values { rows } => {
                for expr in rows.iter_mut().flatten() {
                    self.expression(expr, depth + 1)?;
                }
            }
            QuerySource::Aliased { source, .. } => self.source(source, depth + 1)?,
            QuerySource::TableSample {
                source,
                rate,
                repeatable,
                ..
            } => {
                self.source(source, depth + 1)?;
                self.expression(rate, depth + 1)?;
                if let Some(seed) = repeatable {
                    self.expression(seed, depth + 1)?;
                }
            }
            QuerySource::Subquery { select, .. } => self.select(select, depth + 1)?,
            QuerySource::Join {
                left, right, on, ..
            } => {
                self.source(left, depth + 1)?;
                self.source(right, depth + 1)?;
                self.expression(on, depth + 1)?;
            }
            QuerySource::TableFunction { function, .. } => {
                for expr in function.expressions_mut() {
                    self.expression(expr, depth + 1)?;
                }
            }
            QuerySource::SingleRow | QuerySource::Collection(_) | QuerySource::Cte(_) => {}
        }
        Ok(())
    }

    fn items(&self, items: &mut [SelectItem], depth: usize) -> Result<(), CassieError> {
        self.check(depth)?;
        for item in items {
            match item {
                SelectItem::Expr { expr, .. } => self.expression(expr, depth)?,
                SelectItem::Function { function, alias } => {
                    for expr in function.expressions_mut() {
                        self.expression(expr, depth + 1)?;
                    }
                    if let Some(Coercion {
                        expression,
                        label,
                        _scratch: scratch,
                    }) = self.coerce(function)?
                    {
                        let alias = alias.take().or_else(|| Some(label.to_owned()));
                        *item = SelectItem::Expr {
                            expr: expression,
                            alias,
                        };
                        drop(scratch);
                    }
                }
                SelectItem::WindowFunction { function, .. } => {
                    for expr in function
                        .args
                        .iter_mut()
                        .chain(&mut function.partition_by)
                        .chain(function.order_by.iter_mut().map(|order| &mut order.expr))
                    {
                        self.expression(expr, depth + 1)?;
                    }
                }
                SelectItem::Wildcard | SelectItem::Column { .. } => {}
            }
        }
        Ok(())
    }

    fn expression(&self, expression: &mut Expr, depth: usize) -> Result<(), CassieError> {
        self.check(depth)?;
        if let Expr::Exists(query) = expression {
            return self.statement(query, depth + 1);
        }
        if let Expr::Subquery(subquery) = expression {
            self.statement(&mut subquery.query, depth + 1)?;
        }
        expression.try_visit_children_mut(|child| self.expression(child, depth + 1))?;
        if let Expr::Function(function) = expression {
            if let Some(Coercion {
                expression: coerced,
                _scratch: scratch,
                ..
            }) = self.coerce(function)?
            {
                *expression = coerced;
                drop(scratch);
            }
        }
        Ok(())
    }

    fn coerce(&self, function: &mut FunctionCall) -> Result<Option<Coercion>, CassieError> {
        let Some(target) = crate::sql::functions::primitive_casts::candidate(function) else {
            return Ok(None);
        };
        if !matches!(target.data_type, DataType::Text | DataType::Varchar { .. }) {
            return Ok(None);
        }
        let Expr::Param(index) = function.args[0] else {
            return Ok(None);
        };
        if !matches!(self.declared.get(index).copied(), None | Some(0 | 705)) {
            return Ok(None);
        }
        let label = target.name.rsplit('.').next().expect("native type name");
        // The old Function/argument backing remains live until assignment.
        let scratch = self
            .controls
            .reserve_query_memory(size_of::<Expr>() + label.len())?;
        let expression = Expr::Cast {
            expr: Box::new(
                function
                    .args
                    .pop()
                    .expect("one admitted type-function argument"),
            ),
            data_type: target.data_type,
            source_type: None,
            identity: CastIdentity::Scalar,
        };
        Ok(Some(Coercion {
            expression,
            label,
            _scratch: scratch,
        }))
    }
}

#[cfg(test)]
#[path = "type_functions_tests.rs"]
mod tests;
