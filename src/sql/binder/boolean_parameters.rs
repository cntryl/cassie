//! Checks concrete parameter types at Boolean sinks without rewriting literals.

use std::collections::HashMap;

use super::coalesce_results::ResultTypes;
use super::{
    BinaryOp, BindingContext, CassieError, Catalog, CommonTableExpression, CteQuery, DataType,
    Expr, InsertSource, QuerySource, QueryStatement, SelectItem, SelectSet, SelectStatement,
};
use crate::planner::logical::{LogicalCommand, LogicalPlan};
use crate::sql::ast::{InsertConflictAction, OrderExpr};
use crate::types::Schema;

struct QueryExpressions<'a> {
    source: &'a QuerySource,
    ctes: &'a [CommonTableExpression],
    projection: &'a [SelectItem],
    filter: Option<&'a Expr>,
    distinct_on: &'a [Expr],
    group_by: &'a [Expr],
    having: Option<&'a Expr>,
    order: &'a [OrderExpr],
    set: Option<&'a SelectSet>,
}

impl<'a> QueryExpressions<'a> {
    fn for_plan(plan: &'a LogicalPlan) -> Self {
        Self {
            source: &plan.source,
            ctes: &plan.ctes,
            projection: &plan.projection,
            filter: plan.filter.as_ref(),
            distinct_on: &plan.distinct_on,
            group_by: &plan.group_by,
            having: plan.having.as_ref(),
            order: &plan.order,
            set: plan.set.as_deref(),
        }
    }

    fn for_select(select: &'a SelectStatement) -> Self {
        Self {
            source: &select.source,
            ctes: &select.ctes,
            projection: &select.projection,
            filter: select.filter.as_ref(),
            distinct_on: &select.distinct_on,
            group_by: &select.group_by,
            having: select.having.as_ref(),
            order: &select.order,
            set: select.set.as_deref(),
        }
    }
}

struct ParameterValidation<'a> {
    catalog: &'a Catalog,
    context: &'a BindingContext,
    parameter_types: &'a [i32],
}

pub(crate) fn validate_plan(
    plan: &LogicalPlan,
    catalog: &Catalog,
    context: &BindingContext,
    parameter_types: &[i32],
) -> Result<(), CassieError> {
    if parameter_types.is_empty() {
        return Ok(());
    }
    let validation = ParameterValidation {
        catalog,
        context,
        parameter_types,
    };
    if let Some(command) = &plan.command {
        validation.command(command)?;
    } else {
        validation.query(&QueryExpressions::for_plan(plan), &HashMap::new())?;
    }
    Ok(())
}

impl ParameterValidation<'_> {
    fn scope(
        &self,
        source: &QuerySource,
        ctes: &[CommonTableExpression],
        outer: &HashMap<String, Schema>,
    ) -> Result<ResultTypes, CassieError> {
        ResultTypes::for_scope_with_parameters(
            source,
            ctes,
            self.catalog,
            self.context,
            outer,
            self.parameter_types,
        )
    }

    fn query(
        &self,
        query: &QueryExpressions<'_>,
        outer: &HashMap<String, Schema>,
    ) -> Result<(), CassieError> {
        let types = self.scope(query.source, query.ctes, outer)?;
        self.items(query.projection, &types)?;
        if let Some(filter) = query.filter {
            self.predicate(filter, &types, "WHERE")?;
        }
        if let Some(having) = query.having {
            self.predicate(having, &types, "HAVING")?;
        }
        for expression in query
            .distinct_on
            .iter()
            .chain(query.group_by)
            .chain(query.order.iter().map(|order| &order.expr))
        {
            self.expression(expression, &types)?;
        }
        self.source(query.source, &types)?;
        for cte in query.ctes {
            match &cte.query {
                CteQuery::Simple(statement) => {
                    self.statement(&statement.statement, types.cte_schemas())?;
                }
                CteQuery::Recursive {
                    base, recursive, ..
                } => {
                    self.statement(&base.statement, types.cte_schemas())?;
                    self.statement(&recursive.statement, types.cte_schemas())?;
                }
            }
        }
        if let Some(set) = query.set {
            self.query(
                &QueryExpressions::for_select(&set.right),
                types.cte_schemas(),
            )?;
        }
        Ok(())
    }

    fn statement(
        &self,
        statement: &QueryStatement,
        outer: &HashMap<String, Schema>,
    ) -> Result<(), CassieError> {
        if let QueryStatement::Select(select) = statement {
            self.query(&QueryExpressions::for_select(select), outer)?;
        }
        Ok(())
    }

    fn source(&self, source: &QuerySource, types: &ResultTypes) -> Result<(), CassieError> {
        match source {
            QuerySource::Aliased { source, .. } => self.source(source, types)?,
            QuerySource::Join {
                left, right, on, ..
            } => {
                self.predicate(on, types, "JOIN ON")?;
                self.source(left, types)?;
                self.source(right, types)?;
            }
            QuerySource::Subquery { select, .. } => {
                self.query(&QueryExpressions::for_select(select), types.cte_schemas())?;
            }
            QuerySource::TableFunction { function, .. } => {
                for argument in &function.args {
                    self.expression(argument, types)?;
                }
            }
            QuerySource::Collection(_) | QuerySource::Cte(_) | QuerySource::SingleRow => {}
        }
        Ok(())
    }

    fn items(&self, items: &[SelectItem], types: &ResultTypes) -> Result<(), CassieError> {
        for item in items {
            match item {
                SelectItem::Expr { expr, .. } => self.expression(expr, types)?,
                SelectItem::Function { function, .. } => {
                    for argument in &function.args {
                        self.expression(argument, types)?;
                    }
                }
                SelectItem::WindowFunction { function, .. } => {
                    for expression in function
                        .args
                        .iter()
                        .chain(&function.partition_by)
                        .chain(function.order_by.iter().map(|order| &order.expr))
                    {
                        self.expression(expression, types)?;
                    }
                }
                SelectItem::Wildcard | SelectItem::Column { .. } => {}
            }
        }
        Ok(())
    }

    fn predicate(
        &self,
        expression: &Expr,
        types: &ResultTypes,
        label: &str,
    ) -> Result<(), CassieError> {
        require_boolean_parameter(expression, types, label)?;
        self.expression(expression, types)
    }

    fn expression(&self, expression: &Expr, types: &ResultTypes) -> Result<(), CassieError> {
        match expression {
            Expr::Binary {
                left,
                op: BinaryOp::And | BinaryOp::Or,
                right,
            } => {
                require_boolean_parameter(left, types, "Boolean operator")?;
                require_boolean_parameter(right, types, "Boolean operator")?;
            }
            Expr::Not { expr } => require_boolean_parameter(expr, types, "NOT")?,
            Expr::Case {
                operand: None,
                branches,
                ..
            } => {
                for (condition, _) in branches {
                    require_boolean_parameter(condition, types, "CASE WHEN")?;
                }
            }
            Expr::Exists(statement) => {
                self.statement(&statement.statement, types.cte_schemas())?;
            }
            _ => {}
        }
        expression.try_visit_children(|child| self.expression(child, types))
    }

    fn command(&self, command: &LogicalCommand) -> Result<(), CassieError> {
        match command {
            LogicalCommand::Insert(statement) => {
                let types = self.scope(
                    &QuerySource::Collection(statement.table.clone()),
                    &[],
                    &HashMap::new(),
                )?;
                match &statement.source {
                    InsertSource::Values(rows) => {
                        for expression in rows.iter().flatten() {
                            self.expression(expression, &types)?;
                        }
                    }
                    InsertSource::Select(select) => {
                        self.query(&QueryExpressions::for_select(select), types.cte_schemas())?;
                    }
                }
                if let Some(conflict) = &statement.on_conflict {
                    if let InsertConflictAction::DoUpdate {
                        assignments,
                        filter,
                    } = &conflict.action
                    {
                        let schema =
                            self.catalog.get_schema(&statement.table).ok_or_else(|| {
                                CassieError::CollectionNotFound(statement.table.to_string())
                            })?;
                        let conflict_types = self
                            .scope(
                                &QuerySource::Collection(statement.table.clone()),
                                &[],
                                &HashMap::new(),
                            )?
                            .with_excluded_fields(&schema);
                        self.assignments(assignments, &conflict_types)?;
                        if let Some(filter) = filter {
                            self.predicate(filter, &conflict_types, "ON CONFLICT WHERE")?;
                        }
                    }
                }
                self.items(&statement.returning, &types)?;
            }
            LogicalCommand::Update(statement) => {
                let types = self.scope(
                    &QuerySource::Collection(statement.table.clone()),
                    &[],
                    &HashMap::new(),
                )?;
                self.assignments(&statement.assignments, &types)?;
                if let Some(filter) = &statement.filter {
                    self.predicate(filter, &types, "UPDATE WHERE")?;
                }
                self.items(&statement.returning, &types)?;
            }
            LogicalCommand::Delete(statement) => {
                let types = self.scope(
                    &QuerySource::Collection(statement.table.clone()),
                    &[],
                    &HashMap::new(),
                )?;
                if let Some(filter) = &statement.filter {
                    self.predicate(filter, &types, "DELETE WHERE")?;
                }
                self.items(&statement.returning, &types)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn assignments(
        &self,
        assignments: &[(String, Expr)],
        types: &ResultTypes,
    ) -> Result<(), CassieError> {
        for (_, expression) in assignments {
            self.expression(expression, types)?;
        }
        Ok(())
    }
}

fn require_boolean_parameter(
    expression: &Expr,
    types: &ResultTypes,
    label: &str,
) -> Result<(), CassieError> {
    if !super::validation::expr_contains_parameters(expression) {
        // Literal normalization and parameter-free static validation have their own binder owner.
        return Ok(());
    }
    match types.expression_type(expression) {
        Some(DataType::Boolean | DataType::Null) | None => Ok(()),
        Some(data_type) => Err(CassieError::Planner(format!(
            "{label} expression must have BOOLEAN type (found {data_type:?})"
        ))),
    }
}
