//! Expression outputs are separate from parameter input identities.

use super::check_controls;
use super::{Fold, Frame, ParameterAdmissionError, Summary};
use crate::sql::{ast, binder};

impl Fold<'_, '_> {
    pub(super) fn expression(
        &mut self,
        expr: &ast::Expr,
        frame: &Frame<'_>,
    ) -> Result<Summary, ParameterAdmissionError> {
        let first = || {
            self.summaries
                .values
                .get(frame.start)
                .copied()
                .unwrap_or_else(Summary::unqualified)
        };
        match expr {
            ast::Expr::Param(slot) => {
                self.occurrences
                    .parameter(*slot, self.controls, frame.qualified)
            }
            ast::Expr::StringLiteral(_) | ast::Expr::Null => Ok(Summary::literal()),
            ast::Expr::BoolLiteral(_) => Ok(Summary::known(16)),
            ast::Expr::IntegerLiteral(value) => {
                Ok(Summary::known(oid(&binder::integer_literal_type(*value))))
            }
            ast::Expr::NumberLiteral(_) => Ok(Summary::known(701)),
            ast::Expr::NumericLiteral(_) => Ok(Summary::unqualified()),
            ast::Expr::Column(name) => Ok(self
                .column_oid(frame.context, name)
                .map_or_else(Summary::unqualified, Summary::known)),
            ast::Expr::UnaryNumeric { expr, op } => Ok(
                if matches!(
                    op,
                    ast::UnaryNumericOp::SquareRoot | ast::UnaryNumericOp::CubeRoot
                ) || matches!(
                    op,
                    ast::UnaryNumericOp::Absolute | ast::UnaryNumericOp::Plus
                ) && binder::unary_numeric::is_unknown_operand(expr)
                {
                    Summary::known(701)
                } else {
                    first()
                },
            ),
            ast::Expr::IntegerNegate { width, .. } => Ok(Summary::known(oid(&width.data_type()))),
            ast::Expr::Cast { data_type, .. } => Ok(Summary::known(oid(data_type))),
            ast::Expr::IsNull { .. } | ast::Expr::Not { .. } | ast::Expr::Exists(_) => {
                Ok(Summary::known(16))
            }
            ast::Expr::RowCompare(comparison) => {
                self.row_inputs(comparison, frame)?;
                Ok(Summary::known(16))
            }
            ast::Expr::Function(_) => Ok(first()),
            ast::Expr::Binary { op, left, right } => {
                if matches!(
                    op,
                    ast::BinaryOp::Eq
                        | ast::BinaryOp::NotEq
                        | ast::BinaryOp::IsDistinctFrom
                        | ast::BinaryOp::IsNotDistinctFrom
                        | ast::BinaryOp::Lt
                        | ast::BinaryOp::Lte
                        | ast::BinaryOp::Gt
                        | ast::BinaryOp::Gte
                        | ast::BinaryOp::And
                        | ast::BinaryOp::Or
                ) {
                    self.invalid_boolean_comparison_literals(
                        [left.as_ref(), right.as_ref()],
                        frame,
                    )?;
                }
                self.binary(op, frame)
            }
            ast::Expr::Between {
                expr, low, high, ..
            } => {
                self.invalid_boolean_comparison_literals(
                    [expr.as_ref(), low.as_ref(), high.as_ref()],
                    frame,
                )?;
                self.comparison_inputs(frame)?;
                Ok(Summary::known(16))
            }
            ast::Expr::InList { expr, values, .. } => {
                self.invalid_boolean_comparison_literals(
                    std::iter::once(expr.as_ref()).chain(values.iter()),
                    frame,
                )?;
                self.comparison_inputs(frame)?;
                Ok(Summary::known(16))
            }
            ast::Expr::Case {
                operand,
                branches,
                else_expr,
            } => self.case(operand.as_deref(), branches, else_expr.as_deref(), frame),
        }
    }

    fn binary(
        &mut self,
        op: &ast::BinaryOp,
        frame: &Frame<'_>,
    ) -> Result<Summary, ParameterAdmissionError> {
        if matches!(op, ast::BinaryOp::And | ast::BinaryOp::Or) {
            return Ok(Summary::known(16));
        }
        if *op == ast::BinaryOp::Concat {
            let args = &mut self.summaries.values[frame.start..];
            if args
                .iter()
                .all(|argument| !matches!(argument.oid, 0 | 705 | 25 | 1043))
            {
                self.occurrences.undefined_function("||", 2, None)?;
            }
            for argument in args {
                if matches!(argument.oid, 0 | 705) {
                    self.occurrences.coerce(argument, 25, true)?;
                }
            }
            return Ok(Summary::known(25));
        }
        if let ast::BinaryOp::Bitwise { operation } = op {
            if operation.is_shift() {
                let left = &mut self.summaries.values[frame.start];
                if matches!(left.oid, 0 | 705) {
                    self.occurrences.coerce(left, 23, true)?;
                }
                let result = left.oid;
                let right = &mut self.summaries.values[frame.start + 1];
                if !matches!(right.oid, 0 | 705 | 21 | 23) {
                    self.occurrences
                        .undefined_function(operation.symbol(), 2, Some(right.oid))?;
                }
                self.occurrences.coerce(right, 23, true)?;
                return Ok(Summary::known(result));
            }
        }
        self.comparison_inputs(frame)?;
        Ok(match op {
            ast::BinaryOp::Concat => Summary::known(25),
            ast::BinaryOp::Integer { width, .. } => {
                Summary::known(super::expressions::oid(&width.data_type()))
            }
            ast::BinaryOp::Add
            | ast::BinaryOp::Sub
            | ast::BinaryOp::Mul
            | ast::BinaryOp::Div
            | ast::BinaryOp::Modulo
            | ast::BinaryOp::Bitwise { .. } => {
                let left = self.summaries.values[frame.start];
                let right = self.summaries.values[frame.start + 1];
                common(left, right).map_or_else(Summary::unqualified, |oid| {
                    let mut result = Summary::known(oid);
                    result.qualified = false;
                    result
                })
            }
            ast::BinaryOp::Power
            | ast::BinaryOp::PgvectorCosine
            | ast::BinaryOp::PgvectorL2
            | ast::BinaryOp::PgvectorDot => Summary::known(701),
            ast::BinaryOp::Eq
            | ast::BinaryOp::NotEq
            | ast::BinaryOp::IsDistinctFrom
            | ast::BinaryOp::IsNotDistinctFrom
            | ast::BinaryOp::Lt
            | ast::BinaryOp::Lte
            | ast::BinaryOp::Gt
            | ast::BinaryOp::Gte
            | ast::BinaryOp::Like
            | ast::BinaryOp::And
            | ast::BinaryOp::Or => Summary::known(16),
        })
    }

    fn row_inputs(
        &mut self,
        comparison: &ast::RowComparison,
        frame: &Frame<'_>,
    ) -> Result<(), ParameterAdmissionError> {
        for index in 0..comparison.left.len() {
            check_controls(self.controls)?;
            let indices = [
                frame.start + index,
                frame.start + comparison.left.len() + index,
            ];
            let candidate = indices
                .iter()
                .map(|index| self.summaries.values[*index])
                .find(|summary| summary.oid != 705 && summary.qualified)
                .map_or(25, |summary| summary.oid);
            for (index, expr) in indices
                .into_iter()
                .zip([&comparison.left[index], &comparison.right[index]])
            {
                if candidate == 16 && self.summaries.values[index].oid == 705 {
                    if let ast::Expr::StringLiteral(input) = expr {
                        self.occurrences
                            .invalid_boolean_literal(input, self.controls)?;
                    }
                }
                self.occurrences
                    .coerce(&mut self.summaries.values[index], candidate, true)?;
            }
        }
        Ok(())
    }

    fn comparison_inputs(&mut self, frame: &Frame<'_>) -> Result<(), ParameterAdmissionError> {
        let end = self.summaries.values.len();
        let mut candidate = None;
        for summary in &self.summaries.values[frame.start..end] {
            check_controls(self.controls)?;
            if summary.oid != 705 && summary.qualified {
                candidate = Some(*summary);
                break;
            }
        }
        if let Some(candidate) = candidate {
            for index in frame.start..end {
                check_controls(self.controls)?;
                let summary = &mut self.summaries.values[index];
                self.occurrences.coerce(summary, candidate.oid, true)?;
            }
        }
        Ok(())
    }

    fn invalid_boolean_comparison_literals<'a>(
        &self,
        expressions: impl IntoIterator<Item = &'a ast::Expr>,
        frame: &Frame<'_>,
    ) -> Result<(), ParameterAdmissionError> {
        let summaries = &self.summaries.values[frame.start..];
        if !summaries.iter().any(|summary| summary.oid == 16)
            || summaries
                .iter()
                .any(|summary| !matches!(summary.oid, 16 | 705))
        {
            return Ok(());
        }
        for (expression, summary) in expressions.into_iter().zip(summaries) {
            if summary.oid == 705 {
                if let ast::Expr::StringLiteral(input) = expression {
                    self.occurrences
                        .invalid_boolean_literal(input, self.controls)?;
                }
            }
        }
        Ok(())
    }

    fn case(
        &mut self,
        operand: Option<&ast::Expr>,
        branches: &[(ast::Expr, ast::Expr)],
        else_expr: Option<&ast::Expr>,
        frame: &Frame<'_>,
    ) -> Result<Summary, ParameterAdmissionError> {
        let base = frame.start + usize::from(operand.is_some());
        let operand_oid = operand.and_then(|expr| {
            self.summaries
                .values
                .get(frame.start)
                .filter(|summary| summary.oid != 705)
                .map(|summary| summary.oid)
                .or_else(|| self.known_operand_oid(expr, frame.context))
        });
        if let Some(operand_oid) = operand_oid {
            if operand_oid == 16 {
                for (when, _) in branches {
                    check_controls(self.controls)?;
                    if let ast::Expr::StringLiteral(input) = when {
                        self.occurrences
                            .invalid_boolean_literal(input, self.controls)?;
                    }
                }
            }
            for index in 0..branches.len() {
                check_controls(self.controls)?;
                self.occurrences.coerce(
                    &mut self.summaries.values[base + index * 2],
                    operand_oid,
                    false,
                )?;
            }
        }
        let mut result = Summary::literal();
        for index in 0..branches.len() {
            check_controls(self.controls)?;
            let next = self.summaries.values[base + index * 2 + 1];
            if let Some(oid) = common(result, next) {
                result = Summary::known(oid);
            }
        }
        if else_expr.is_some() {
            if let Some(oid) = common(result, *self.summaries.values.last().expect("ELSE summary"))
            {
                result = Summary::known(oid);
            }
        }
        if result.oid == 16 {
            for (_, value) in branches {
                check_controls(self.controls)?;
                if let ast::Expr::StringLiteral(input) = value {
                    self.occurrences
                        .invalid_boolean_literal(input, self.controls)?;
                }
            }
            if let Some(ast::Expr::StringLiteral(input)) = else_expr {
                self.occurrences
                    .invalid_boolean_literal(input, self.controls)?;
            }
        }
        if result.oid != 705 {
            for index in 0..branches.len() {
                check_controls(self.controls)?;
                self.occurrences.coerce(
                    &mut self.summaries.values[base + index * 2 + 1],
                    result.oid,
                    false,
                )?;
            }
            if else_expr.is_some() {
                self.occurrences.coerce(
                    self.summaries.values.last_mut().expect("ELSE summary"),
                    result.oid,
                    false,
                )?;
            }
        }
        result.qualified = false;
        Ok(result)
    }
}

pub(super) fn oid(data_type: &crate::types::DataType) -> i32 {
    i32::try_from(data_type.type_oid()).unwrap_or(i32::MAX)
}

pub(super) fn common(left: Summary, right: Summary) -> Option<i32> {
    // Equal and UNKNOWN identity need no owned DataType, including arrays.
    if left.oid == right.oid || right.oid == 705 {
        return Some(left.oid);
    }
    if left.oid == 705 {
        return Some(right.oid);
    }
    let data_type = |summary: Summary| {
        if summary.oid == 705 {
            Some(crate::types::DataType::Null)
        } else if super::super::occurrences::primitive_oid(summary.oid) {
            binder::parameter_data_type_for_oid(summary.oid)
        } else {
            None
        }
    };
    binder::common_case_type(data_type(left)?, data_type(right)?).map(|data_type| oid(&data_type))
}
