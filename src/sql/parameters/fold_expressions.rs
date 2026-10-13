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
            ast::Expr::Column(name) => Ok(self
                .column_oid(frame.context, name)
                .map_or_else(Summary::unqualified, Summary::known)),
            ast::Expr::Cast { data_type, .. } => Ok(Summary::known(oid(data_type))),
            ast::Expr::IsNull { .. } | ast::Expr::Not { .. } | ast::Expr::Exists(_) => {
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
        self.comparison_inputs(frame)?;
        Ok(match op {
            ast::BinaryOp::Add | ast::BinaryOp::Sub | ast::BinaryOp::Mul | ast::BinaryOp::Div => {
                let left = self.summaries.values[frame.start];
                let right = self.summaries.values[frame.start + 1];
                common(left, right).map_or_else(Summary::unqualified, |oid| {
                    let mut result = Summary::known(oid);
                    result.qualified = false;
                    result
                })
            }
            ast::BinaryOp::PgvectorCosine
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
