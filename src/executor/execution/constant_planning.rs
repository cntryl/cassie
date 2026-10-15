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

use super::exists_target::ExistsClauses;
use super::{filter, Expr, FunctionMeta, HashMap, LogicalCommand, LogicalPlan, QueryError};
use super::{CommonTableExpression, CteQuery, HashSet, InsertSource, QuerySource};
use super::{QueryStatement, SelectItem};
use crate::executor::filter::ScalarValue;
use crate::sql::ast::{
    BinaryOp, FunctionCall, InsertConflictAction, OrderExpr, ParsedStatement, SelectSet,
    SelectStatement,
};
use crate::sql::functions::FunctionId;

/// Reports the first error PostgreSQL would raise while folding the
/// constant expressions of `plan`.
///
/// # Errors
///
/// Returns the folding error, such as division by zero.
pub(super) fn check_plan(plan: &LogicalPlan) -> Result<(), QueryError> {
    let folder = Folder::default();
    folder.check_select(&SelectParts {
        ctes: &plan.ctes,
        source: &plan.source,
        projection: &plan.projection,
        filter: plan.filter.as_ref(),
        group_by: &plan.group_by,
        having: plan.having.as_ref(),
        order: &plan.order,
        distinct_on: &plan.distinct_on,
        set: plan.set.as_deref(),
    })?;
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
    /// Only constant inputs; evaluating the original expression follows the
    /// same pruned path, so evaluation is deferred to the outermost such node.
    Constant,
    /// A Boolean (`None` is NULL) decided by short-circuit folding; the
    /// original expression may still contain non-constant inputs.
    Decided(Option<bool>),
    /// Reads a row, a parameter, a subquery or a non-immutable function.
    Variable,
}

/// A folded condition: a constant truth value (`None` is NULL), or unknown
/// until run time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Truth {
    Known(Option<bool>),
    Unknown,
}

struct SelectParts<'a> {
    ctes: &'a [CommonTableExpression],
    source: &'a QuerySource,
    projection: &'a [SelectItem],
    filter: Option<&'a Expr>,
    group_by: &'a [Expr],
    having: Option<&'a Expr>,
    order: &'a [OrderExpr],
    distinct_on: &'a [Expr],
    set: Option<&'a SelectSet>,
}

impl<'a> SelectParts<'a> {
    fn of(select: &'a SelectStatement) -> Self {
        Self {
            ctes: &select.ctes,
            source: &select.source,
            projection: &select.projection,
            filter: select.filter.as_ref(),
            group_by: &select.group_by,
            having: select.having.as_ref(),
            order: &select.order,
            distinct_on: &select.distinct_on,
            set: select.set.as_deref(),
        }
    }
}

#[derive(Default)]
struct Folder {
    no_functions: HashMap<String, FunctionMeta>,
}

impl Folder {
    fn check_select(&self, select: &SelectParts<'_>) -> Result<(), QueryError> {
        self.check_ctes(select)?;
        self.check_source(select.source)?;
        self.check_items(select.projection)?;
        self.check_all(select.filter)?;
        self.check_all(select.group_by)?;
        self.check_all(select.having)?;
        self.check_all(select.order.iter().map(|order| &order.expr))?;
        self.check_all(select.distinct_on)?;
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
        let mut referenced = HashSet::new();
        source_names(select.source, &mut referenced);
        if let Some(set) = select.set {
            select_names(&set.right, &mut referenced);
        }
        for cte in select.ctes {
            cte_names(&cte.query, &mut referenced);
        }
        for cte in select.ctes {
            if !referenced.contains(&cte.name.to_ascii_lowercase()) {
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
                    if fold == Fold::Constant {
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
        if fold == Fold::Constant && !is_literal(expr) {
            self.evaluate(expr)?;
        }
        Ok(())
    }

    /// Evaluates a constant expression. A folding error is the planning
    /// error; any other failure means the evaluator cannot fold this constant,
    /// which is left to run time.
    fn evaluate(&self, expr: &Expr) -> Result<Option<ScalarValue>, QueryError> {
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
        match expr {
            Expr::Column(_) | Expr::Param(_) => Ok(Fold::Variable),
            Expr::Exists(statement) => {
                self.check_exists(statement)?;
                Ok(Fold::Variable)
            }
            Expr::StringLiteral(_)
            | Expr::NumberLiteral(_)
            | Expr::IntegerLiteral(_)
            | Expr::BoolLiteral(_)
            | Expr::Null => Ok(Fold::Constant),
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
            Expr::Cast { expr, .. } => self.fold_operands([&**expr], true, false),
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
        let constant = immutable && folded.iter().all(|(fold, _)| *fold == Fold::Constant);
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
        if left_fold == Fold::Constant && right_fold == Fold::Constant {
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
        // short-circuited operand is compared through its decided literal, so
        // the original CASE is no longer safe to evaluate as a whole.
        let mut deferrable = true;
        let operand = match operand {
            Some(operand) => {
                let fold = self.fold(operand)?;
                self.settle(fold, operand)?;
                Some(match fold {
                    Fold::Constant => Some(operand.clone()),
                    Fold::Decided(value) => {
                        deferrable = false;
                        Some(value.map_or(Expr::Null, Expr::BoolLiteral))
                    }
                    Fold::Variable => None,
                })
            }
            None => None,
        };
        let mut variable = false;
        for (when, then) in branches {
            let when_fold = self.fold(when)?;
            let condition = match &operand {
                None => {
                    deferrable &= !matches!(when_fold, Fold::Decided(_));
                    self.truth(when_fold, when)?
                }
                Some(Some(operand)) if when_fold == Fold::Constant => {
                    self.truth(Fold::Constant, &simple_case_match(operand, when))?
                }
                Some(_) => {
                    self.settle(when_fold, when)?;
                    Truth::Unknown
                }
            };
            match condition {
                Truth::Known(Some(false) | None) => {}
                Truth::Known(Some(true)) => return self.case_result(then, variable, deferrable),
                Truth::Unknown => {
                    variable = true;
                    self.check(then)?;
                }
            }
        }
        match else_expr {
            Some(else_expr) => self.case_result(else_expr, variable, deferrable),
            None if variable => Ok(Fold::Variable),
            None => Ok(Fold::Constant),
        }
    }

    fn case_result(
        &self,
        result: &Expr,
        variable: bool,
        deferrable: bool,
    ) -> Result<Fold, QueryError> {
        let fold = self.fold(result)?;
        if fold == Fold::Constant && (variable || !deferrable) {
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
            Some(
                FunctionId::Length
                | FunctionId::Lower
                | FunctionId::Upper
                | FunctionId::Substring
                | FunctionId::Trim
                | FunctionId::Ltrim
                | FunctionId::Rtrim
                | FunctionId::SplitPart
                | FunctionId::Strpos
                | FunctionId::NullIf
                | FunctionId::Greatest
                | FunctionId::Least
                | FunctionId::Abs,
            ) => self.fold_operands(&function.args, true, false),
            _ => self.fold_operands(&function.args, false, false),
        }
    }

    /// COALESCE drops NULL constants and stops at its first non-NULL
    /// constant; inputs after it are never processed.
    fn fold_coalesce(&self, args: &[Expr]) -> Result<Fold, QueryError> {
        let mut variable = false;
        let mut deferrable = true;
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
                Some(true) => deferrable &= fold == Fold::Constant,
                Some(false) if variable => return Ok(Fold::Variable),
                Some(false) => {
                    return Ok(match fold {
                        Fold::Constant if !deferrable => Fold::Variable,
                        fold => fold,
                    });
                }
                None => variable = true,
            }
        }
        Ok(if variable || !deferrable {
            Fold::Variable
        } else {
            Fold::Constant
        })
    }
}

/// Collects the relation names a FROM tree reads, through nested subqueries.
fn source_names(source: &QuerySource, names: &mut HashSet<String>) {
    match source {
        QuerySource::Cte(name) => {
            names.insert(name.to_ascii_lowercase());
        }
        QuerySource::Collection(path) => {
            if let Some(name) = path.values().last() {
                names.insert(name.to_ascii_lowercase());
            }
        }
        QuerySource::Aliased { source, .. } => source_names(source, names),
        QuerySource::Subquery { select, .. } => select_names(select, names),
        QuerySource::Join { left, right, .. } => {
            source_names(left, names);
            source_names(right, names);
        }
        QuerySource::TableFunction { .. } | QuerySource::SingleRow => {}
    }
}

fn select_names(select: &SelectStatement, names: &mut HashSet<String>) {
    source_names(&select.source, names);
    if let Some(set) = &select.set {
        select_names(&set.right, names);
    }
    for cte in &select.ctes {
        cte_names(&cte.query, names);
    }
}

fn cte_names(query: &CteQuery, names: &mut HashSet<String>) {
    let statements: [Option<&ParsedStatement>; 2] = match query {
        CteQuery::Simple(statement) => [Some(statement), None],
        CteQuery::Recursive {
            base, recursive, ..
        } => [Some(base), Some(recursive)],
    };
    for statement in statements.into_iter().flatten() {
        if let QueryStatement::Select(select) = &statement.statement {
            select_names(select, names);
        }
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
            | Expr::BoolLiteral(_)
            | Expr::Null
    )
}

fn is_folding_error(error: &QueryError) -> bool {
    matches!(
        error,
        QueryError::General(message)
            if message == "division by zero"
                || message == filter::BIGINT_OUT_OF_RANGE
                || message == crate::types::numeric::FLOAT_OVERFLOW
    )
}
