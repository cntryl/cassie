//! Versioned admission of the selected finite durable expression contract.

use super::ast::{
    BinaryOp, CteQuery, Expr, InsertConflictAction, InsertSource, ParsedStatement, QuerySource,
    QueryStatement, SelectItem, SelectStatement,
};
use crate::declarations::{ConstraintOperator, FieldConstraint};

mod check_comparison;
pub use check_comparison::{case_operand, exact_eq, field_type, literal, original_operand, validate};

#[cfg(test)]
mod tests;

/// Rejection of a parsed or stored definition under its persisted semantics version.
#[derive(Debug)]
pub enum DefinitionError {
    Unsupported(String),
    Execution(String),
}

impl std::fmt::Display for DefinitionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported(message) => write!(formatter, "unsupported feature: {message}"),
            Self::Execution(message) => write!(formatter, "execution error: {message}"),
        }
    }
}

impl std::error::Error for DefinitionError {}

const DIAGNOSTIC: &str = "IS [NOT] DISTINCT FROM is not supported in persisted definitions; legacy definition semantics do not admit this operator";

#[derive(Clone, Copy)]
struct Policy(u32);

#[must_use]
pub fn is_legacy<T: Default + PartialEq>(version: &T) -> bool {
    *version == T::default()
}

/// Checks that a persisted semantics version is readable.
///
/// # Errors
///
/// Returns a definition error for an unknown semantics version.
pub fn version(version: u32) -> Result<(), DefinitionError> {
    if version <= 1 {
        Ok(())
    } else {
        Err(DefinitionError::Unsupported(format!(
            "unknown persisted definition semantics version {version}"
        )))
    }
}

/// One selected comparison of scalar literals. Binding, coercion and folding
/// belong to the ordinary binder, rather than a parser-specific equality law.
#[must_use]
pub fn null_safe_default(sql: &str) -> bool {
    fn literal(expr: &Expr) -> bool {
        match expr {
            Expr::Null
            | Expr::BoolLiteral(_)
            | Expr::IntegerLiteral(_)
            | Expr::NumberLiteral(_)
            | Expr::StringLiteral(_) => true,
            Expr::Cast {
                expr,
                data_type:
                    crate::types::DataType::SmallInt
                    | crate::types::DataType::Int
                    | crate::types::DataType::BigInt
                    | crate::types::DataType::Float
                    | crate::types::DataType::Boolean
                    | crate::types::DataType::Text,
                ..
            } => literal(expr),
            _ => false,
        }
    }
    let Ok(Expr::Binary {
        left,
        op: BinaryOp::IsDistinctFrom | BinaryOp::IsNotDistinctFrom,
        right,
    }) = super::parser::parse_expression(sql)
    else {
        return false;
    };
    literal(&left) && literal(&right)
}

/// Validates a parsed expression under the current persisted semantics.
///
/// # Errors
///
/// Returns a definition error when the syntax is unsupported.
pub fn expression<A>(expr: &Expr<A>) -> Result<(), DefinitionError> {
    Policy(1).expression(expr)
}

/// # Errors
///
/// Returns a definition error when the requested version or syntax is unsupported.
pub fn expression_sql(sql: &str) -> Result<(), DefinitionError> {
    Policy(1).expression_sql(sql)
}

/// # Errors
///
/// Returns a definition error when the requested version or syntax is unsupported.
pub fn query_sql(sql: &str) -> Result<(), DefinitionError> {
    Policy(1).query_sql(sql)
}

/// # Errors
///
/// Returns a definition error when the requested version or syntax is unsupported.
pub fn statement<A>(query: &ParsedStatement<A>) -> Result<(), DefinitionError> {
    Policy(1).statement(query)
}

/// # Errors
///
/// Returns a definition error when the requested version or syntax is unsupported.
pub fn create_rollup<A>(
    definition: &super::ast::CreateRollupStatement<A>,
) -> Result<(), DefinitionError> {
    Policy(1).create_rollup(definition)
}

/// # Errors
///
/// Returns a definition error when the requested version or syntax is unsupported.
pub fn encoded_expression(sql: &str) -> Result<(), DefinitionError> {
    Policy(1).encoded_expression(sql)
}

/// # Errors
///
/// Returns a definition error when the requested version or syntax is unsupported.
pub fn legacy_expression<A>(expr: &Expr<A>) -> Result<(), DefinitionError> {
    Policy(0).expression(expr)
}

/// # Errors
///
/// Returns a definition error when the requested version or syntax is unsupported.
pub fn legacy_expression_sql(sql: &str) -> Result<(), DefinitionError> {
    Policy(0).expression_sql(sql)
}

/// # Errors
///
/// Returns a definition error when the requested version or syntax is unsupported.
pub fn legacy_query_sql(sql: &str) -> Result<(), DefinitionError> {
    Policy(0).query_sql(sql)
}

/// # Errors
///
/// Returns a definition error when the requested version or syntax is unsupported.
pub fn legacy_statement<A>(query: &ParsedStatement<A>) -> Result<(), DefinitionError> {
    Policy(0).statement(query)
}

/// # Errors
///
/// Returns a definition error when the requested version or syntax is unsupported.
pub fn constraints<A>(constraints: &[FieldConstraint<A>]) -> Result<(), DefinitionError> {
    for constraint in constraints {
        version(constraint.definition_semantics_version)?;
        Policy(constraint.definition_semantics_version)
            .constraints(std::slice::from_ref(constraint))?;
    }
    Ok(())
}

/// # Errors
///
/// Returns a definition error when the requested version or syntax is unsupported.
pub fn index(
    semantics_version: u32,
    expressions: &[String],
    predicate: Option<&str>,
) -> Result<(), DefinitionError> {
    version(semantics_version)?;
    let policy = Policy(semantics_version);
    for expression in expressions.iter().map(String::as_str).chain(predicate) {
        policy.encoded_expression(expression)?;
    }
    Ok(())
}

/// # Errors
///
/// Returns a definition error when the requested version or syntax is unsupported.
pub fn query(semantics_version: u32, sql: &str) -> Result<(), DefinitionError> {
    version(semantics_version)?;
    Policy(semantics_version).query_sql(sql)
}

/// # Errors
///
/// Returns a definition error when the requested version or syntax is unsupported.
pub fn rollup<'a>(
    semantics_version: u32,
    bucket_expr: &str,
    expressions: impl IntoIterator<Item = &'a str>,
) -> Result<(), DefinitionError> {
    version(semantics_version)?;
    let policy = Policy(semantics_version);
    policy.expression_sql(bucket_expr)?;
    for expression in expressions {
        policy.expression_sql(expression)?;
        if let Ok(Expr::Function(function)) = super::parser::parse_expression(expression) {
            Policy::reject_rollup_modifiers(&function)?;
        }
    }
    Ok(())
}

impl Policy {
    fn expression<A>(self, expr: &Expr<A>) -> Result<(), DefinitionError> {
        if let Expr::Function(function) = expr {
            if crate::functions::function(&function.name)
                .is_some_and(|metadata| !metadata.is_aggregate)
            {
                let modifier = if function.is_distinct() {
                    Some("DISTINCT")
                } else if !function.order_by.is_empty() {
                    Some("ORDER BY")
                } else if function.filter.is_some() {
                    Some("FILTER")
                } else {
                    None
                };
                if let Some(modifier) = modifier {
                    return Err(DefinitionError::Execution(format!(
                        "{modifier} specified, but {} is not an aggregate function",
                        function.name.to_ascii_lowercase()
                    )));
                }
            }
        }
        if self.0 == 0 && matches!(expr, Expr::RowCompare(comparison) if comparison.op.null_safe())
        {
            return Err(DefinitionError::Unsupported(DIAGNOSTIC.into()));
        }
        if self.0 == 0
            && matches!(
                expr,
                Expr::Binary {
                    op: BinaryOp::IsDistinctFrom | BinaryOp::IsNotDistinctFrom,
                    ..
                }
            )
        {
            return Err(DefinitionError::Unsupported(DIAGNOSTIC.into()));
        }
        if let Some(query) = expr.subquery_statement() {
            self.statement(query)?;
        }
        expr.try_visit_children(|expr| self.expression(expr))
    }

    fn expression_sql(self, sql: &str) -> Result<(), DefinitionError> {
        // Existing callers retain their own malformed-definition diagnostics.
        if let Ok(expr) = super::parser::parse_expression(sql) {
            self.expression(&expr)?;
        }
        Ok(())
    }

    fn query_sql(self, sql: &str) -> Result<(), DefinitionError> {
        match super::parse_statement(sql) {
            Ok(query) => self.statement(&query),
            Err(error)
                if error.kind() == super::SqlErrorKind::Unsupported
                    && error
                        .message()
                        .strip_prefix("unsupported feature: ")
                        .unwrap_or(error.message())
                        == DIAGNOSTIC =>
            {
                Err(DefinitionError::Unsupported(DIAGNOSTIC.into()))
            }
            // Other malformed/unsupported definitions retain their existing caller diagnostics.
            Err(_) => Ok(()),
        }
    }

    fn statement<A>(self, query: &ParsedStatement<A>) -> Result<(), DefinitionError> {
        match &query.statement {
            QueryStatement::CreateTable(definition) => {
                for field in &definition.fields {
                    self.constraints(&field.constraints)?;
                }
                if let Some(query) = &definition.as_query {
                    self.statement(&query.query)?;
                }
            }
            QueryStatement::AlterTable(definition) => self.alter_table(&definition.operation)?,
            QueryStatement::CreateView(definition) => self.query_sql(&definition.query)?,
            QueryStatement::CreateFunction(definition) => {
                Policy(0).expression_sql(&definition.body)?;
            }
            QueryStatement::CreateProcedure(definition) => Policy(0).query_sql(&definition.body)?,
            QueryStatement::CreateMaterializedProjection(definition) => {
                self.query_sql(&definition.query)?;
            }
            QueryStatement::CreateMaterializedView(definition) => {
                self.query_sql(&definition.query)?;
            }
            QueryStatement::CreateIndex(definition) => {
                for expr in definition.expressions.iter().chain(&definition.predicate) {
                    self.expression(expr)?;
                }
            }
            QueryStatement::CreateRollup(definition) => self.create_rollup(definition)?,
            QueryStatement::CallProcedure(call) => {
                for expr in &call.args {
                    self.expression(expr)?;
                }
            }
            QueryStatement::Select(query) => self.select(query)?,
            QueryStatement::Explain(query) => self.statement(&query.statement)?,
            QueryStatement::Insert(insert) => {
                match &insert.source {
                    InsertSource::Values(rows) => {
                        for expr in rows.iter().flatten() {
                            self.expression(expr)?;
                        }
                    }
                    InsertSource::Select(query) => self.select(query)?,
                }
                if let Some(conflict) = &insert.on_conflict {
                    if let InsertConflictAction::DoUpdate {
                        assignments,
                        filter,
                        ..
                    } = &conflict.action
                    {
                        for (_, expr) in assignments {
                            self.expression(expr)?;
                        }
                        if let Some(expr) = filter {
                            self.expression(expr)?;
                        }
                    }
                }
                self.items(&insert.returning)?;
            }
            QueryStatement::Update(update) => {
                for (_, expr) in &update.assignments {
                    self.expression(expr)?;
                }
                if let Some(expr) = &update.filter {
                    self.expression(expr)?;
                }
                self.items(&update.returning)?;
            }
            QueryStatement::Delete(delete) => {
                if let Some(expr) = &delete.filter {
                    self.expression(expr)?;
                }
                self.items(&delete.returning)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn alter_table<A>(
        self,
        operation: &super::ast::AlterTableOperation<A>,
    ) -> Result<(), DefinitionError> {
        use super::ast::AlterTableOperation;
        match operation {
            AlterTableOperation::Multiple { operations } => {
                for operation in operations {
                    self.alter_table(operation)?;
                }
            }
            AlterTableOperation::AddColumn { constraints, .. }
            | AlterTableOperation::AddConstraint { constraints } => {
                self.constraints(constraints)?;
            }
            AlterTableOperation::AlterColumnSetDefault {
                default_expression: Some(expression),
                ..
            } => self.expression_sql(expression)?,
            AlterTableOperation::AlterColumnType { using, .. } => self.expression(using)?,
            _ => {}
        }
        Ok(())
    }

    fn select<A>(self, query: &SelectStatement<A>) -> Result<(), DefinitionError> {
        self.source(&query.source)?;
        for cte in &query.ctes {
            match &cte.query {
                CteQuery::Simple(query) => self.statement(query)?,
                CteQuery::Recursive {
                    base, recursive, ..
                } => {
                    self.statement(base)?;
                    self.statement(recursive)?;
                }
            }
        }
        if let Some(set) = &query.set {
            self.select(&set.right)?;
        }
        self.items(&query.projection)?;
        for expr in query
            .filter
            .iter()
            .chain(&query.having)
            .chain(&query.group_by)
            .chain(&query.distinct_on)
            .chain(&query.limit)
            .chain(&query.offset)
        {
            self.expression(expr)?;
        }
        for order in &query.order {
            self.expression(&order.expr)?;
        }
        Ok(())
    }

    fn source<A>(self, query: &QuerySource<A>) -> Result<(), DefinitionError> {
        match query {
            QuerySource::Values { rows } => rows
                .iter()
                .flatten()
                .try_for_each(|expr| self.expression(expr))?,

            QuerySource::Aliased { source: query, .. } => self.source(query)?,
            QuerySource::Subquery { select: query, .. } => self.select(query)?,
            QuerySource::Join {
                left, right, on, ..
            } => {
                self.source(left)?;
                self.source(right)?;
                self.expression(on)?;
            }
            QuerySource::TableFunction { function, form, .. } => {
                for expr in function
                    .expressions()
                    .chain(form.iter().flat_map(|form| form.graph_expressions()))
                {
                    self.expression(expr)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn items<A>(self, projection: &[SelectItem<A>]) -> Result<(), DefinitionError> {
        for item in projection {
            match item {
                SelectItem::Expr { expr, .. } => self.expression(expr)?,
                SelectItem::Function { function, .. } => {
                    for expr in function.expressions() {
                        self.expression(expr)?;
                    }
                }
                SelectItem::WindowFunction { function, .. } => {
                    for expr in function
                        .args
                        .iter()
                        .chain(function.filter.as_deref())
                        .chain(&function.partition_by)
                    {
                        self.expression(expr)?;
                    }
                    for order in &function.order_by {
                        self.expression(&order.expr)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn encoded_expression(self, sql: &str) -> Result<(), DefinitionError> {
        if let Ok(expr) = serde_json::from_str::<Expr<()>>(sql) {
            if expr.any_descendant_or_self(&mut |expr| {
                matches!(expr, Expr::Exists(_) | Expr::Subquery(_))
            }) {
                return Err(DefinitionError::Unsupported(
                    "index expressions cannot contain subqueries".into(),
                ));
            }
            self.expression(&expr)?;
        }
        Ok(())
    }

    fn constraints<A>(
        self,
        constraints: &[FieldConstraint<A>],
    ) -> Result<(), DefinitionError> {
        for constraint in constraints {
            if let Some(check) = &constraint.check {
                if self.0 == 0 && check.bound_expression.is_some() {
                    return Err(DefinitionError::Unsupported(
                        "bound CHECK comparisons require versioned definition semantics".into(),
                    ));
                }
                if self.0 == 1
                    && (check.bound_expression.is_some()
                        || matches!(
                            check.operator,
                            ConstraintOperator::IsDistinctFrom
                                | ConstraintOperator::IsNotDistinctFrom
                        ))
                {
                    validate(check)?;
                }
            }
            if self.0 == 0
                && constraint.check.as_ref().is_some_and(|check| {
                    matches!(
                        check.operator,
                        ConstraintOperator::IsDistinctFrom
                            | ConstraintOperator::IsNotDistinctFrom
                    )
                })
            {
                return Err(DefinitionError::Unsupported(DIAGNOSTIC.into()));
            }
            if let Some(expression) = &constraint.default_expression {
                self.expression_sql(expression)?;
                if self.0 == 1
                    && constraint.default_value.is_none()
                    && constraint.default_sequence.is_none()
                    && crate::default_expression::parse_volatile_default_expression(expression).is_none()
                {
                    return Err(DefinitionError::Unsupported(
                        "versioned constant DEFAULT metadata requires its bound literal value"
                            .into(),
                    ));
                }
            }
        }
        Ok(())
    }

    fn create_rollup<A>(
        self,
        definition: &super::ast::CreateRollupStatement<A>,
    ) -> Result<(), DefinitionError> {
        for item in &definition.aggregates {
            if let SelectItem::Function { function, .. } = item {
                Self::reject_rollup_modifiers(function)?;
            }
        }
        for expr in definition
            .bucket
            .args
            .iter()
            .chain(&definition.group_by)
            .chain(&definition.filter)
        {
            self.expression(expr)?;
        }
        self.items(&definition.aggregates)
    }

    fn reject_rollup_modifiers<A>(
        function: &super::ast::FunctionCall<A>,
    ) -> Result<(), DefinitionError> {
        if function.is_distinct() || function.filter.is_some() || !function.order_by.is_empty() {
            return Err(DefinitionError::Unsupported(
                "aggregate modifiers are not supported in incremental rollup definitions".into(),
            ));
        }
        Ok(())
    }
}
