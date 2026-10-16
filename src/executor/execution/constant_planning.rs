//! Planning-time constant expression errors, after PostgreSQL's
//! `eval_const_expressions`.
//!
//! PostgreSQL folds every immutable expression whose inputs are all constants
//! while it plans a statement, so `1/0` fails even in a dynamically dead CASE
//! branch or a command that reaches no row. Folding follows argument order and
//! prunes as it goes: a constant FALSE or NULL `WHEN` drops its arm unprocessed,
//! a constant TRUE `WHEN` ends the CASE, a deciding constant AND/OR input skips
//! the inputs after it, and COALESCE stops at its first non-NULL constant.
//! Row values, parameters, subqueries and non-immutable functions are not
//! constant, so expressions that read them keep their runtime laziness.
//!
//! The plan is not rewritten; this walk only reports the arithmetic error that
//! folding raises. Constant subtrees are evaluated with the scalar evaluator,
//! once, at the outermost node that still folds. An `EXISTS` subquery is
//! checked after discarding the clauses PostgreSQL drops from it.

use super::cte_reachability::{self, SelectParts};
use super::exists_target::ExistsClauses;
use super::{filter, Expr, FunctionMeta, HashMap, LogicalCommand, LogicalPlan, QueryError};
use super::{CteQuery, InsertSource, QuerySource};
use super::{QueryStatement, SelectItem};
use crate::executor::filter::ScalarValue;
use crate::sql::ast::{
    BinaryOp, FunctionCall, InsertConflictAction, ParsedStatement, SelectStatement,
};
use crate::sql::functions::FunctionId;

/// Reports the first error PostgreSQL would raise while folding the
/// constant expressions of `plan`.
///
/// # Errors
///
/// Returns the folding error, such as division by zero.
pub(crate) fn check_plan(
    plan: &LogicalPlan,
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<(), QueryError> {
    super::check_timeout(controls)?;
    let folder = Folder {
        no_functions: HashMap::new(),
        controls,
    };
    folder.check_select(&SelectParts::plan(plan))?;
    match &plan.command {
        Some(LogicalCommand::Insert(insert)) => {
            match &insert.source {
                InsertSource::Values(rows) => folder.check_all(rows.iter().flatten())?,
                InsertSource::Select(select) => folder.check_statement_select(select)?,
            }
            if let Some(InsertConflictAction::DoUpdate {
                assignments,
                filter,
            }) = insert.on_conflict.as_ref().map(|clause| &clause.action)
            {
                folder.check_all(assignments.iter().map(|(_, expr)| expr))?;
                folder.check_all(filter)?;
            }
            folder.check_items(&insert.returning)
        }
        Some(LogicalCommand::Update(update)) => {
            folder.check_all(update.assignments.iter().map(|(_, expr)| expr))?;
            folder.check_all(&update.filter)?;
            folder.check_items(&update.returning)
        }
        Some(LogicalCommand::Delete(delete)) => {
            folder.check_all(&delete.filter)?;
            folder.check_items(&delete.returning)
        }
        _ => Ok(()),
    }
}

/// What folding learned about an expression.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fold {
    /// Semantically constant inputs, including already-pruned descendants.
    /// Evaluate through the shared semantic-constant evaluator first.
    Constant,
    /// A Boolean (`None` is NULL) decided by short-circuit folding; the
    /// original expression may still contain non-constant inputs.
    Decided(Option<bool>),
    /// Reads a row, a parameter, a subquery or a non-immutable function.
    Variable,
}

impl Fold {
    fn is_known(self) -> bool {
        !matches!(self, Self::Variable)
    }
}

/// A folded condition: a constant truth value (`None` is NULL), or unknown
/// until run time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Truth {
    Known(Option<bool>),
    Unknown,
}

struct Folder<'a> {
    controls: &'a crate::runtime::QueryExecutionControls,
    no_functions: HashMap<String, FunctionMeta>,
}

impl Folder<'_> {
    fn check_select(&self, select: &SelectParts<'_>) -> Result<(), QueryError> {
        self.check_ctes(select)?;
        self.check_source(select.source)?;
        self.check_items(select.projection)?;
        self.check_all(select.filter)?;
        self.check_all(select.group_by)?;
        self.check_all(select.having)?;
        self.check_all(select.order.iter().map(|order| &order.expr))?;
        self.check_all(select.distinct_on)?;
        for bound in select.limit.into_iter().chain(select.offset) {
            crate::sql::pagination::check_constants(bound, self.controls)?;
        }
        if let Some(set) = select.set {
            self.check_statement_select(&set.right)?;
        }
        Ok(())
    }

    fn check_statement_select(&self, select: &SelectStatement) -> Result<(), QueryError> {
        self.check_select(&SelectParts::of(select))
    }

    /// PostgreSQL plans only the `WITH` queries that the statement references.
    fn check_ctes(&self, select: &SelectParts<'_>) -> Result<(), QueryError> {
        if select.ctes.is_empty() {
            return Ok(());
        }
        let live = cte_reachability::used(select, self.controls)?;
        for (index, cte) in select.ctes.iter().enumerate() {
            if !live.contains(index) {
                continue;
            }
            match &cte.query {
                CteQuery::Simple(statement) => self.check_parsed_select(statement)?,
                CteQuery::Recursive {
                    base, recursive, ..
                } => {
                    self.check_parsed_select(base)?;
                    self.check_parsed_select(recursive)?;
                }
            }
        }
        Ok(())
    }

    fn check_parsed_select(&self, statement: &ParsedStatement) -> Result<(), QueryError> {
        match &statement.statement {
            QueryStatement::Select(select) => self.check_statement_select(select),
            _ => Ok(()),
        }
    }

    fn check_source(&self, source: &QuerySource) -> Result<(), QueryError> {
        match source {
            QuerySource::Values { rows } => {
                for expression in rows.iter().flatten() {
                    super::check_timeout(self.controls)?;
                    let _scratch =
                        self.controls
                            .reserve_query_memory(super::source::values_scratch_bytes(
                                expression,
                                &[],
                            )?)?;
                    self.check(expression)?;
                }
                Ok(())
            }

            QuerySource::Aliased { source, .. } => self.check_source(source),
            QuerySource::TableFunction { function, .. } => self.check_all(function.expressions()),
            QuerySource::Subquery { select, .. } => self.check_statement_select(select),
            QuerySource::Join {
                left, right, on, ..
            } => {
                self.check_source(left)?;
                self.check_source(right)?;
                self.check(on)
            }
            QuerySource::Collection(_) | QuerySource::Cte(_) | QuerySource::SingleRow => Ok(()),
        }
    }

    /// PostgreSQL plans a reached `EXISTS` subquery, but only after dropping
    /// the clauses that cannot change whether it yields a row.
    fn check_exists(&self, statement: &ParsedStatement) -> Result<(), QueryError> {
        let QueryStatement::Select(select) = &statement.statement else {
            return Ok(());
        };
        if ExistsClauses::of_select(select).target_is_irrelevant() {
            self.check_ctes(&SelectParts::of(select))?;
            self.check_source(&select.source)?;
            self.check_all(&select.filter)
        } else {
            self.check_statement_select(select)
        }
    }

    fn check_items(&self, items: &[SelectItem]) -> Result<(), QueryError> {
        for item in items {
            match item {
                SelectItem::Wildcard | SelectItem::Column { .. } => {}
                SelectItem::Expr { expr, .. } => self.check(expr)?,
                SelectItem::Function { function, .. } => {
                    let fold = self.fold_function(function)?;
                    if fold.is_known() {
                        self.evaluate(&Expr::Function(Box::new(function.clone())))?;
                    }
                }
                SelectItem::WindowFunction { function, .. } => {
                    self.check_all(&function.args)?;
                    self.check_all(&function.partition_by)?;
                    self.check_all(function.order_by.iter().map(|order| &order.expr))?;
                }
            }
        }
        Ok(())
    }

    fn check_all<'e>(&self, exprs: impl IntoIterator<Item = &'e Expr>) -> Result<(), QueryError> {
        exprs.into_iter().try_for_each(|expr| self.check(expr))
    }

    fn check(&self, expr: &Expr) -> Result<(), QueryError> {
        let fold = self.fold(expr)?;
        self.settle(fold, expr)
    }

    /// Evaluates a constant expression whose parent does not fold.
    fn settle(&self, fold: Fold, expr: &Expr) -> Result<(), QueryError> {
        if fold.is_known() && !is_literal(expr) {
            self.evaluate(expr)?;
        }
        Ok(())
    }

    /// Evaluates a constant expression. A folding error is the planning
    /// error; any other failure means the evaluator cannot fold this constant,
    /// which is left to run time.
    fn evaluate(&self, expr: &Expr) -> Result<Option<ScalarValue>, QueryError> {
        let _character_memory = self
            .controls
            .reserve_query_memory(filter::character_scratch_bytes(expr)?)?;
        match filter::constant_scalar(expr) {
            Ok(Some(value)) => return Ok(Some(value)),
            Err(error) if is_folding_error(&error) => return Err(error),
            Ok(None) | Err(_) => {}
        }
        let row: Vec<(String, crate::types::Value)> = Vec::new();
        match filter::eval_scalar(&row, expr, &[], None, &self.no_functions, None, None) {
            Ok(value) => Ok(Some(value)),
            Err(error) if is_folding_error(&error) => Err(error),
            Err(_) => Ok(None),
        }
    }

    /// The constant truth value of a folded condition, when folding decides it.
    fn truth(&self, fold: Fold, expr: &Expr) -> Result<Truth, QueryError> {
        Ok(match fold {
            Fold::Decided(value) => Truth::Known(value),
            Fold::Constant => match self.evaluate(expr)? {
                Some(ScalarValue::Bool(value)) => Truth::Known(Some(value)),
                Some(ScalarValue::Null) => Truth::Known(None),
                _ => Truth::Unknown,
            },
            Fold::Variable => Truth::Unknown,
        })
    }

    fn fold(&self, expr: &Expr) -> Result<Fold, QueryError> {
        super::check_timeout(self.controls)?;
        match expr {
            Expr::Column(_) | Expr::Param(_) => Ok(Fold::Variable),
            Expr::Exists(statement) => {
                self.check_exists(statement)?;
                Ok(Fold::Variable)
            }
            Expr::StringLiteral(_)
            | Expr::NumberLiteral(_)
            | Expr::IntegerLiteral(_)
            | Expr::NumericLiteral(_)
            | Expr::BoolLiteral(_)
            | Expr::Null => Ok(Fold::Constant),
            Expr::RowCompare(comparison) => self.fold_row(comparison),
            Expr::Binary { left, op, right } => match op {
                BinaryOp::And => self.fold_logical(left, right, false),
                BinaryOp::Or => self.fold_logical(left, right, true),
                // pgvector distances are left to the vector evaluator.
                BinaryOp::PgvectorCosine | BinaryOp::PgvectorL2 | BinaryOp::PgvectorDot => {
                    self.fold_operands([&**left, &**right], false, false)
                }
                _ => self.fold_operands([&**left, &**right], true, false),
            },
            Expr::IsNull { expr, negated } => Ok(match self.fold(expr)? {
                Fold::Decided(value) => Fold::Decided(Some(value.is_none() != *negated)),
                fold => fold,
            }),
            Expr::Not { expr } => Ok(match self.fold(expr)? {
                Fold::Decided(value) => Fold::Decided(value.map(|value| !value)),
                fold => fold,
            }),
            Expr::Cast { expr, .. }
            | Expr::UnaryNumeric { expr, .. }
            | Expr::IntegerNegate { expr, .. } => self.fold_operands([&**expr], true, false),
            // `x IN (...)` folds as `x = ANY(ARRAY[...])`: every element folds,
            // although the runtime list test stops at its first match.
            Expr::InList { expr, values, .. } => {
                self.fold_operands(std::iter::once(&**expr).chain(values), true, true)
            }
            Expr::Between {
                expr,
                low,
                high,
                negated,
            } => self.fold_between(expr, low, high, *negated),
            Expr::Case {
                operand,
                branches,
                else_expr,
            } => self.fold_case(operand.as_deref(), branches, else_expr.as_deref()),
            Expr::Function(function) => self.fold_function(function),
        }
    }

    fn fold_row(&self, comparison: &crate::sql::ast::RowComparison) -> Result<Fold, QueryError> {
        use crate::sql::ast::RowComparisonOp;
        let equality = matches!(comparison.op, RowComparisonOp::Eq | RowComparisonOp::NotEq)
            || comparison.op.null_safe();
        if !equality {
            let folded =
                self.fold_operands(comparison.left.iter().chain(&comparison.right), true, true)?;
            // PostgreSQL retains multi-field RowCompareExpr nodes after
            // folding arguments. Singletons lower to scalar operators.
            return Ok(if comparison.left.len() == 1 {
                folded
            } else {
                Fold::Variable
            });
        }
        let is_or = matches!(
            comparison.op,
            RowComparisonOp::NotEq | RowComparisonOp::IsDistinctFrom
        );
        let mut variable = false;
        let mut has_null = false;
        for (left, right) in comparison.left.iter().zip(&comparison.right) {
            super::check_timeout(self.controls)?;
            let left_fold = self.fold(left)?;
            let right_fold = self.fold(right)?;
            if left_fold != Fold::Variable && right_fold != Fold::Variable {
                if let (Some(left), Some(right)) = (
                    self.folded_value(left_fold, left)?,
                    self.folded_value(right_fold, right)?,
                ) {
                    match filter::row_field_result(&left, comparison.op, &right)? {
                        ScalarValue::Bool(value) if value == is_or => {
                            return Ok(Fold::Decided(Some(value)))
                        }
                        ScalarValue::Null => has_null = true,
                        ScalarValue::Bool(_) => {}
                        _ => variable = true,
                    }
                    continue;
                }
            }
            self.settle(left_fold, left)?;
            self.settle(right_fold, right)?;
            variable = true;
        }
        Ok(combined(variable, has_null, is_or))
    }

    fn folded_value(
        &self,
        fold: Fold,
        expression: &Expr,
    ) -> Result<Option<ScalarValue>, QueryError> {
        match fold {
            Fold::Constant => self.evaluate(expression),
            Fold::Decided(value) => Ok(Some(value.map_or(ScalarValue::Null, ScalarValue::Bool))),
            Fold::Variable => Ok(None),
        }
    }

    /// Folds a strict operator or immutable function: constant when every
    /// input is constant; otherwise each constant input is evaluated now.
    fn fold_operands<'e>(
        &self,
        operands: impl IntoIterator<Item = &'e Expr>,
        immutable: bool,
        settle_constants: bool,
    ) -> Result<Fold, QueryError> {
        let mut folded = Vec::new();
        for operand in operands {
            folded.push((self.fold(operand)?, operand));
        }
        let constant = immutable && folded.iter().all(|(fold, _)| fold.is_known());
        if !constant || settle_constants {
            for (fold, operand) in folded {
                self.settle(fold, operand)?;
            }
        }
        Ok(if constant {
            Fold::Constant
        } else {
            Fold::Variable
        })
    }

    /// AND/OR fold their inputs in order and stop at the first constant that
    /// decides the result (FALSE for AND, TRUE for OR), even after a
    /// non-constant input.
    fn fold_logical(&self, left: &Expr, right: &Expr, is_or: bool) -> Result<Fold, QueryError> {
        let mut has_null = false;
        let mut variable = false;
        for input in [left, right] {
            let fold = self.fold(input)?;
            match self.truth(fold, input)? {
                Truth::Known(Some(value)) if value == is_or => {
                    return Ok(Fold::Decided(Some(is_or)))
                }
                Truth::Known(Some(_)) => {}
                Truth::Known(None) => has_null = true,
                Truth::Unknown => variable = true,
            }
        }
        Ok(combined(variable, has_null, is_or))
    }

    /// `a BETWEEN b AND c` folds as `a >= b AND a <= c`, and NOT BETWEEN as
    /// `a < b OR a > c`, so a deciding first comparison skips `c`.
    fn fold_between(
        &self,
        expr: &Expr,
        low: &Expr,
        high: &Expr,
        negated: bool,
    ) -> Result<Fold, QueryError> {
        let (first, second) = if negated {
            (BinaryOp::Lt, BinaryOp::Gt)
        } else {
            (BinaryOp::Gte, BinaryOp::Lte)
        };
        let value = self.fold(expr)?;
        let low_fold = self.fold(low)?;
        let mut has_null = false;
        let mut variable = false;
        for (op, bound, bound_fold) in [(first, low, Some(low_fold)), (second, high, None)] {
            let bound_fold = match bound_fold {
                Some(fold) => fold,
                None => self.fold(bound)?,
            };
            match self.compare(value, expr, op, bound_fold, bound)? {
                Truth::Known(Some(result)) if result == negated => {
                    return Ok(Fold::Decided(Some(negated)));
                }
                Truth::Known(Some(_)) => {}
                Truth::Known(None) => has_null = true,
                Truth::Unknown => variable = true,
            }
        }
        Ok(combined(variable, has_null, negated))
    }

    fn compare(
        &self,
        left_fold: Fold,
        left: &Expr,
        op: BinaryOp,
        right_fold: Fold,
        right: &Expr,
    ) -> Result<Truth, QueryError> {
        if left_fold.is_known() && right_fold.is_known() {
            let comparison = Expr::Binary {
                left: Box::new(left.clone()),
                op,
                right: Box::new(right.clone()),
            };
            return self.truth(Fold::Constant, &comparison);
        }
        self.settle(left_fold, left)?;
        self.settle(right_fold, right)?;
        Ok(Truth::Unknown)
    }

    /// CASE drops arms whose condition folds to FALSE or NULL without
    /// processing their result, and a condition that folds to TRUE makes its
    /// result the CASE result, so later arms and ELSE are never processed.
    fn fold_case(
        &self,
        operand: Option<&Expr>,
        branches: &[(Expr, Expr)],
        else_expr: Option<&Expr>,
    ) -> Result<Fold, QueryError> {
        // A simple CASE compares a constant operand with each WHEN value. A
        // short-circuited operand is compared through its decided literal.
        // The semantic evaluator preserves that pruning through parent casts.
        let operand = match operand {
            Some(operand) => {
                let fold = self.fold(operand)?;
                self.settle(fold, operand)?;
                Some(match fold {
                    Fold::Constant => Some(operand.clone()),
                    Fold::Decided(value) => Some(value.map_or(Expr::Null, Expr::BoolLiteral)),
                    Fold::Variable => None,
                })
            }
            None => None,
        };
        let mut variable = false;
        for (when, then) in branches {
            let when_fold = self.fold(when)?;
            let condition = match &operand {
                None => self.truth(when_fold, when)?,
                Some(Some(operand)) if when_fold.is_known() => {
                    self.truth(Fold::Constant, &simple_case_match(operand, when))?
                }
                Some(_) => {
                    self.settle(when_fold, when)?;
                    Truth::Unknown
                }
            };
            match condition {
                Truth::Known(Some(false) | None) => {}
                Truth::Known(Some(true)) => return self.case_result(then, variable),
                Truth::Unknown => {
                    variable = true;
                    self.check(then)?;
                }
            }
        }
        match else_expr {
            Some(else_expr) => self.case_result(else_expr, variable),
            None if variable => Ok(Fold::Variable),
            None => Ok(Fold::Constant),
        }
    }

    fn case_result(&self, result: &Expr, variable: bool) -> Result<Fold, QueryError> {
        let fold = self.fold(result)?;
        if fold.is_known() && variable {
            self.settle(fold, result)?;
            return Ok(Fold::Variable);
        }
        Ok(if variable { Fold::Variable } else { fold })
    }

    fn fold_function(&self, function: &FunctionCall) -> Result<Fold, QueryError> {
        self.check_all(function.filter.as_deref())?;
        self.check_all(function.order_by.iter().map(|order| &order.expr))?;
        let id = crate::sql::functions::function(&function.name).map(|metadata| metadata.id);
        match id {
            Some(FunctionId::Coalesce) => self.fold_coalesce(&function.args),
            Some(id) if crate::sql::functions::is_immutable_scalar(id) => {
                self.fold_operands(&function.args, true, false)
            }
            _ => self.fold_operands(&function.args, false, false),
        }
    }

    /// COALESCE drops NULL constants and stops at its first non-NULL
    /// constant; inputs after it are never processed.
    fn fold_coalesce(&self, args: &[Expr]) -> Result<Fold, QueryError> {
        let mut variable = false;
        for arg in args {
            let fold = self.fold(arg)?;
            let is_null = match fold {
                Fold::Decided(value) => Some(value.is_none()),
                Fold::Constant => self
                    .evaluate(arg)?
                    .map(|value| matches!(value, ScalarValue::Null)),
                Fold::Variable => None,
            };
            match is_null {
                Some(true) => {}
                Some(false) if variable => return Ok(Fold::Variable),
                Some(false) => {
                    return Ok(fold);
                }
                None => variable = true,
            }
        }
        Ok(if variable {
            Fold::Variable
        } else {
            Fold::Constant
        })
    }
}

fn combined(variable: bool, has_null: bool, is_or: bool) -> Fold {
    if variable {
        Fold::Variable
    } else if has_null {
        Fold::Decided(None)
    } else {
        Fold::Decided(Some(!is_or))
    }
}

fn simple_case_match(operand: &Expr, when: &Expr) -> Expr {
    Expr::Case {
        operand: Some(Box::new(operand.clone())),
        branches: vec![(when.clone(), Expr::BoolLiteral(true))],
        else_expr: Some(Box::new(Expr::BoolLiteral(false))),
    }
}

const fn is_literal(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::StringLiteral(_)
            | Expr::NumberLiteral(_)
            | Expr::IntegerLiteral(_)
            | Expr::NumericLiteral(_)
            | Expr::BoolLiteral(_)
            | Expr::Null
    )
}

fn is_folding_error(error: &QueryError) -> bool {
    matches!(
        error,
        QueryError::Cassie(
            crate::app::CassieError::FloatInput(_) | crate::app::CassieError::BooleanInput(_)
        )
    ) || matches!(
        error,
        QueryError::General(message)
            if message == "division by zero"
                || matches!(message.as_str(), "smallint out of range" | "integer out of range" | "invalid input syntax for type smallint" | "invalid input syntax for type integer" | "invalid input syntax for type bigint")
                || message == filter::BIGINT_OUT_OF_RANGE
                || message == crate::types::numeric::FLOAT_OVERFLOW
    )
}

#[cfg(test)]
#[path = "constant_planning_controls_tests.rs"]
mod controls_tests;
