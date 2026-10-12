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
            ast::Expr::Column(name) => Ok(crate::sql::field_type_for_column(
                self.fields(frame.context),
                name,
            )
            .map_or_else(Summary::unqualified, |data_type| {
                Summary::known(oid(data_type))
            })),
            ast::Expr::Cast { data_type, .. } => Ok(Summary::known(oid(data_type))),
            ast::Expr::IsNull { .. } | ast::Expr::Not { .. } | ast::Expr::Exists(_) => {
                Ok(Summary::known(16))
            }
            ast::Expr::Function(_) => Ok(first()),
            ast::Expr::Binary { op, .. } => self.binary(op, frame),
            ast::Expr::Between { .. } | ast::Expr::InList { .. } => {
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

    fn case(
        &mut self,
        operand: Option<&ast::Expr>,
        branches: &[(ast::Expr, ast::Expr)],
        else_expr: Option<&ast::Expr>,
        frame: &Frame<'_>,
    ) -> Result<Summary, ParameterAdmissionError> {
        let base = frame.start + usize::from(operand.is_some());
        if let Some(operand_oid) =
            operand.and_then(|expr| known_operand_oid(expr, self.fields(frame.context)))
        {
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

// This is the OID projection of known_expr_type's existing operand cases.
// Borrow nonprimitive Column/Cast types instead of cloning an Array Box.
fn known_operand_oid(expr: &ast::Expr, fields: &crate::sql::FieldTypeMap) -> Option<i32> {
    match expr {
        ast::Expr::Column(name) => crate::sql::field_type_for_column(fields, name).map(oid),
        ast::Expr::Cast { data_type, .. } => Some(oid(data_type)),
        ast::Expr::StringLiteral(_) => Some(25),
        ast::Expr::NumberLiteral(_) => Some(701),
        ast::Expr::IntegerLiteral(value) => Some(oid(&binder::integer_literal_type(*value))),
        ast::Expr::BoolLiteral(_) => Some(16),
        ast::Expr::Null => Some(oid(&crate::types::DataType::Null)),
        ast::Expr::Param(_)
        | ast::Expr::Binary { .. }
        | ast::Expr::Between { .. }
        | ast::Expr::InList { .. }
        | ast::Expr::IsNull { .. }
        | ast::Expr::Not { .. }
        | ast::Expr::Function(_)
        | ast::Expr::Exists(_)
        | ast::Expr::Case { .. } => None,
    }
}
