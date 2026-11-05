//! Versioned admission of the selected finite durable expression contract.

use super::ast::{
    BinaryOp, CteQuery, Expr, InsertConflictAction, InsertSource, ParsedStatement, QuerySource,
    QueryStatement, SelectItem, SelectStatement,
};
use crate::app::CassieError;

const DIAGNOSTIC: &str = "IS [NOT] DISTINCT FROM is not supported in persisted definitions; legacy definition semantics do not admit this operator";

#[derive(Clone, Copy)]
struct Policy(u32);

pub(crate) fn is_legacy<T: Default + PartialEq>(version: &T) -> bool {
    *version == T::default()
}

pub(crate) fn version(version: u32) -> Result<(), CassieError> {
    if version <= 1 {
        Ok(())
    } else {
        Err(CassieError::Unsupported(format!(
            "unknown persisted definition semantics version {version}"
        )))
    }
}

/// One selected comparison of scalar literals. Binding, coercion and folding
/// belong to the ordinary binder, rather than a parser-specific equality law.
pub(crate) fn null_safe_default(sql: &str) -> bool {
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

pub(crate) fn expression(expr: &Expr) -> Result<(), CassieError> {
    Policy(1).expression(expr)
}
pub(crate) fn expression_sql(sql: &str) -> Result<(), CassieError> {
    Policy(1).expression_sql(sql)
}
pub(crate) fn query_sql(sql: &str) -> Result<(), CassieError> {
    Policy(1).query_sql(sql)
}
pub(crate) fn statement(query: &ParsedStatement) -> Result<(), CassieError> {
    Policy(1).statement(query)
}

pub(crate) fn create_rollup(
    definition: &super::ast::CreateRollupStatement,
) -> Result<(), CassieError> {
    Policy(1).create_rollup(definition)
}

#[cfg(test)]
pub(crate) fn encoded_expression(sql: &str) -> Result<(), CassieError> {
    Policy(1).encoded_expression(sql)
}

pub(crate) fn legacy_expression(expr: &Expr) -> Result<(), CassieError> {
    Policy(0).expression(expr)
}
pub(crate) fn legacy_expression_sql(sql: &str) -> Result<(), CassieError> {
    Policy(0).expression_sql(sql)
}
pub(crate) fn legacy_query_sql(sql: &str) -> Result<(), CassieError> {
    Policy(0).query_sql(sql)
}
pub(crate) fn legacy_statement(query: &ParsedStatement) -> Result<(), CassieError> {
    Policy(0).statement(query)
}
pub(crate) fn constraints(
    constraints: &[crate::catalog::FieldConstraint],
) -> Result<(), CassieError> {
    for constraint in constraints {
        version(constraint.definition_semantics_version)?;
        Policy(constraint.definition_semantics_version)
            .constraints(std::slice::from_ref(constraint))?;
    }
    Ok(())
}
pub(crate) fn index(metadata: &crate::catalog::IndexMeta) -> Result<(), CassieError> {
    version(metadata.definition_semantics_version)?;
    Policy(metadata.definition_semantics_version).index(metadata)
}
pub(crate) fn view(metadata: &crate::catalog::ViewMeta) -> Result<(), CassieError> {
    version(metadata.definition_semantics_version)?;
    Policy(metadata.definition_semantics_version).query_sql(&metadata.query)
}
pub(crate) fn projection(
    metadata: &crate::catalog::MaterializedProjectionMeta,
) -> Result<(), CassieError> {
    version(metadata.definition_semantics_version)?;
    Policy(metadata.definition_semantics_version).query_sql(&metadata.query)
}
pub(crate) fn rollup(metadata: &crate::catalog::RollupMeta) -> Result<(), CassieError> {
    version(metadata.definition_semantics_version)?;
    Policy(metadata.definition_semantics_version).rollup(metadata)
}

impl Policy {
    fn expression(self, expr: &Expr) -> Result<(), CassieError> {
        if let Expr::Function(function) = expr {
            if crate::sql::functions::function(&function.name)
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
                    return Err(CassieError::Execution(format!(
                        "{modifier} specified, but {} is not an aggregate function",
                        function.name.to_ascii_lowercase()
                    )));
                }
            }
        }
        if self.0 == 0 && matches!(expr, Expr::RowCompare(comparison) if comparison.op.null_safe())
        {
            return Err(CassieError::Unsupported(DIAGNOSTIC.into()));
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
            return Err(CassieError::Unsupported(DIAGNOSTIC.into()));
        }
        if let Some(query) = expr.subquery_statement() {
            self.statement(query)?;
        }
        expr.try_visit_children(|expr| self.expression(expr))
    }

    fn expression_sql(self, sql: &str) -> Result<(), CassieError> {
        // Existing callers retain their own malformed-definition diagnostics.
        if let Ok(expr) = super::parser::parse_expression(sql) {
            self.expression(&expr)?;
        }
        Ok(())
    }

    fn query_sql(self, sql: &str) -> Result<(), CassieError> {
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
                Err(CassieError::Unsupported(DIAGNOSTIC.into()))
            }
            // Other malformed/unsupported definitions retain their existing caller diagnostics.
            Err(_) => Ok(()),
        }
    }

    fn statement(self, query: &ParsedStatement) -> Result<(), CassieError> {
        match &query.statement {
            QueryStatement::CreateTable(definition) => {
                for field in &definition.fields {
                    self.constraints(&field.constraints)?;
                }
                if let Some(query) = &definition.as_query {
                    self.statement(&query.query)?;
                }
            }
            QueryStatement::CreateGraph(definition) => {
                for field in definition.node_fields.iter().chain(&definition.edge_fields) {
                    self.constraints(&field.constraints)?;
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

    fn alter_table(self, operation: &super::ast::AlterTableOperation) -> Result<(), CassieError> {
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

    fn select(self, query: &SelectStatement) -> Result<(), CassieError> {
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

    fn source(self, query: &QuerySource) -> Result<(), CassieError> {
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
            QuerySource::TableFunction { function, .. } => {
                for expr in function.expressions() {
                    self.expression(expr)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn items(self, projection: &[SelectItem]) -> Result<(), CassieError> {
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

    fn encoded_expression(self, sql: &str) -> Result<(), CassieError> {
        if let Ok(expr) = serde_json::from_str::<Expr>(sql) {
            if expr.any_descendant_or_self(&mut |expr| {
                matches!(expr, Expr::Exists(_) | Expr::Subquery(_))
            }) {
                return Err(CassieError::Unsupported(
                    "index expressions cannot contain subqueries".into(),
                ));
            }
            self.expression(&expr)?;
        }
        Ok(())
    }

    fn constraints(
        self,
        constraints: &[crate::catalog::FieldConstraint],
    ) -> Result<(), CassieError> {
        for constraint in constraints {
            if self.0 == 0
                && constraint.check.as_ref().is_some_and(|check| {
                    matches!(
                        check.operator,
                        crate::catalog::ConstraintOperator::IsDistinctFrom
                            | crate::catalog::ConstraintOperator::IsNotDistinctFrom
                    )
                })
            {
                return Err(CassieError::Unsupported(DIAGNOSTIC.into()));
            }
            if let Some(expression) = &constraint.default_expression {
                self.expression_sql(expression)?;
                if self.0 == 1
                    && constraint.default_value.is_none()
                    && constraint.default_sequence.is_none()
                    && crate::catalog::parse_volatile_default_expression(expression).is_none()
                {
                    return Err(CassieError::Unsupported(
                        "versioned constant DEFAULT metadata requires its bound literal value"
                            .into(),
                    ));
                }
            }
        }
        Ok(())
    }

    fn index(self, metadata: &crate::catalog::IndexMeta) -> Result<(), CassieError> {
        for expression in metadata.expressions.iter().chain(&metadata.predicate) {
            self.encoded_expression(expression)?;
        }
        Ok(())
    }

    fn rollup(self, metadata: &crate::catalog::RollupMeta) -> Result<(), CassieError> {
        self.expression_sql(&metadata.bucket_expr)?;
        for expression in metadata
            .group_keys
            .iter()
            .chain(
                metadata
                    .aggregates
                    .iter()
                    .map(|aggregate| &aggregate.expression),
            )
            .chain(&metadata.filter_expr)
        {
            self.expression_sql(expression)?;
            if let Ok(Expr::Function(function)) = super::parser::parse_expression(expression) {
                Self::reject_rollup_modifiers(&function)?;
            }
        }
        Ok(())
    }

    fn create_rollup(
        self,
        definition: &super::ast::CreateRollupStatement,
    ) -> Result<(), CassieError> {
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

    fn reject_rollup_modifiers(function: &super::ast::FunctionCall) -> Result<(), CassieError> {
        if function.is_distinct() || function.filter.is_some() || !function.order_by.is_empty() {
            return Err(CassieError::Unsupported(
                "aggregate modifiers are not supported in incremental rollup definitions".into(),
            ));
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn should_reject_scalar_aggregate_modifiers_given_a_persisted_expression_when_definition_admission_runs(
    ) {
        // Arrange
        let cases = ["LOWER(label) FILTER (WHERE TRUE)", "LOWER(DISTINCT label)"];

        // Act
        for sql in cases {
            let expression =
                crate::sql::parser::parse_expression(sql).expect("function clause syntax");
            let error = super::expression(&expression)
                .expect_err("scalar modifiers cannot enter persisted definitions");

            // Assert
            assert_eq!(error.descriptor().sql_state, "42809");
        }
        let control =
            crate::sql::parser::parse_expression("LOWER(ALL label)").expect("valid ALL control");
        assert!(super::expression(&control).is_ok());
    }
}

#[cfg(test)]
#[path = "definition_guard_window_tests.rs"]
mod window_tests;
