//! Structural traversal helpers for [`Expr`].
//!
//! Expression walkers that only need to recurse into sub-expressions should
//! use these helpers instead of re-listing every variant, so adding a variant
//! (or a new child such as a CASE branch) only touches this module.

use std::ops::ControlFlow;

use super::ast_query::Expr;

impl Expr {
    /// Visits each direct sub-expression in evaluation order, stopping early
    /// when `visit` breaks. Subqueries inside `EXISTS` are not visited.
    pub fn try_for_each_child<'a, B>(
        &'a self,
        mut visit: impl FnMut(&'a Expr) -> ControlFlow<B>,
    ) -> ControlFlow<B> {
        match self {
            Self::Case {
                operand,
                branches,
                else_expr,
            } => {
                if let Some(operand) = operand {
                    visit(operand)?;
                }
                for (when, then) in branches {
                    visit(when)?;
                    visit(then)?;
                }
                if let Some(else_expr) = else_expr {
                    visit(else_expr)?;
                }
            }
            Self::Binary { left, right, .. } => {
                visit(left)?;
                visit(right)?;
            }
            Self::IsNull { expr, .. } | Self::Not { expr } | Self::Cast { expr, .. } => {
                visit(expr)?;
            }
            Self::InList { expr, values, .. } => {
                visit(expr)?;
                for value in values {
                    visit(value)?;
                }
            }
            Self::Between {
                expr, low, high, ..
            } => {
                visit(expr)?;
                visit(low)?;
                visit(high)?;
            }
            Self::Function(function) => {
                for arg in &function.args {
                    visit(arg)?;
                }
            }
            Self::Column(_)
            | Self::Param(_)
            | Self::StringLiteral(_)
            | Self::NumberLiteral(_)
            | Self::IntegerLiteral(_)
            | Self::BoolLiteral(_)
            | Self::Null
            | Self::Exists(_) => {}
        }
        ControlFlow::Continue(())
    }

    /// Rebuilds this expression with every direct sub-expression replaced by
    /// `map(child)`. `EXISTS` subqueries and leaves are cloned unchanged.
    #[must_use]
    pub fn map_children(&self, mut map: impl FnMut(&Expr) -> Expr) -> Expr {
        match self {
            Self::Case {
                operand,
                branches,
                else_expr,
            } => Self::Case {
                operand: operand.as_deref().map(|expr| Box::new(map(expr))),
                branches: branches
                    .iter()
                    .map(|(when, then)| (map(when), map(then)))
                    .collect(),
                else_expr: else_expr.as_deref().map(|expr| Box::new(map(expr))),
            },
            Self::Binary { left, op, right } => Self::Binary {
                left: Box::new(map(left)),
                op: op.clone(),
                right: Box::new(map(right)),
            },
            Self::IsNull { expr, negated } => Self::IsNull {
                expr: Box::new(map(expr)),
                negated: *negated,
            },
            Self::InList {
                expr,
                values,
                negated,
            } => Self::InList {
                expr: Box::new(map(expr)),
                values: values.iter().map(&mut map).collect(),
                negated: *negated,
            },
            Self::Between {
                expr,
                low,
                high,
                negated,
            } => Self::Between {
                expr: Box::new(map(expr)),
                low: Box::new(map(low)),
                high: Box::new(map(high)),
                negated: *negated,
            },
            Self::Not { expr } => Self::Not {
                expr: Box::new(map(expr)),
            },
            Self::Cast { expr, data_type } => Self::Cast {
                expr: Box::new(map(expr)),
                data_type: data_type.clone(),
            },
            Self::Function(function) => Self::Function(super::ast_query::FunctionCall {
                name: function.name.clone(),
                args: function.args.iter().map(&mut map).collect(),
            }),
            Self::Column(_)
            | Self::Param(_)
            | Self::StringLiteral(_)
            | Self::NumberLiteral(_)
            | Self::IntegerLiteral(_)
            | Self::BoolLiteral(_)
            | Self::Null
            | Self::Exists(_) => self.clone(),
        }
    }

    /// Visits each direct sub-expression in evaluation order.
    pub fn for_each_child<'a>(&'a self, mut visit: impl FnMut(&'a Expr)) {
        let _ = self.try_for_each_child(|child| {
            visit(child);
            ControlFlow::<()>::Continue(())
        });
    }

    /// Visits each direct sub-expression, returning the first error.
    ///
    /// # Errors
    ///
    /// Returns the first error produced by `visit`.
    pub fn try_visit_children<'a, E>(
        &'a self,
        mut visit: impl FnMut(&'a Expr) -> Result<(), E>,
    ) -> Result<(), E> {
        match self.try_for_each_child(|child| match visit(child) {
            Ok(()) => ControlFlow::Continue(()),
            Err(error) => ControlFlow::Break(error),
        }) {
            ControlFlow::Continue(()) => Ok(()),
            ControlFlow::Break(error) => Err(error),
        }
    }

    /// Returns the first `Some` produced by `visit` over direct sub-expressions.
    pub fn find_map_child<'a, T>(
        &'a self,
        mut visit: impl FnMut(&'a Expr) -> Option<T>,
    ) -> Option<T> {
        match self.try_for_each_child(|child| {
            visit(child).map_or(ControlFlow::Continue(()), ControlFlow::Break)
        }) {
            ControlFlow::Continue(()) => None,
            ControlFlow::Break(value) => Some(value),
        }
    }

    /// Returns whether any direct sub-expression satisfies `predicate`.
    pub fn any_child(&self, mut predicate: impl FnMut(&Expr) -> bool) -> bool {
        self.find_map_child(|child| predicate(child).then_some(()))
            .is_some()
    }

    /// Returns whether every direct sub-expression satisfies `predicate`.
    pub fn all_children(&self, mut predicate: impl FnMut(&Expr) -> bool) -> bool {
        !self.any_child(|child| !predicate(child))
    }

    /// Compares two expressions structurally, ignoring ASCII case in column
    /// and function names, so `GROUP BY` matching does not depend on how the
    /// query spelled identifiers.
    #[must_use]
    pub fn structurally_eq(&self, other: &Expr) -> bool {
        match (self, other) {
            (Self::Column(left), Self::Column(right)) => left.eq_ignore_ascii_case(right),
            (Self::Param(left), Self::Param(right)) => left == right,
            (Self::StringLiteral(left), Self::StringLiteral(right)) => left == right,
            (Self::NumberLiteral(left), Self::NumberLiteral(right)) => {
                left.to_bits() == right.to_bits()
            }
            (Self::IntegerLiteral(left), Self::IntegerLiteral(right)) => left == right,
            (Self::BoolLiteral(left), Self::BoolLiteral(right)) => left == right,
            (Self::Null, Self::Null) => true,
            (Self::Function(left), Self::Function(right)) => {
                left.name.eq_ignore_ascii_case(&right.name)
                    && all_structurally_eq(&left.args, &right.args)
            }
            (Self::Exists(left), Self::Exists(right)) => {
                format!("{left:?}") == format!("{right:?}")
            }
            _ => composite_structurally_eq(self, other),
        }
    }

    /// Returns whether this expression or any descendant satisfies `predicate`.
    /// `EXISTS` subqueries are not searched.
    pub fn any_descendant_or_self(&self, predicate: &mut impl FnMut(&Expr) -> bool) -> bool {
        predicate(self) || self.any_child(|child| child.any_descendant_or_self(predicate))
    }
}

/// Compares the expression forms that carry sub-expressions: their shape,
/// operator, and flags must match before their children are compared.
fn composite_structurally_eq(left: &Expr, right: &Expr) -> bool {
    match (left, right) {
        (
            Expr::Case {
                operand: left_operand,
                branches: left_branches,
                else_expr: left_else,
            },
            Expr::Case {
                operand: right_operand,
                branches: right_branches,
                else_expr: right_else,
            },
        ) => {
            optional_structurally_eq(left_operand.as_deref(), right_operand.as_deref())
                && left_branches.len() == right_branches.len()
                && left_branches.iter().zip(right_branches).all(
                    |((left_when, left_then), (right_when, right_then))| {
                        left_when.structurally_eq(right_when)
                            && left_then.structurally_eq(right_then)
                    },
                )
                && optional_structurally_eq(left_else.as_deref(), right_else.as_deref())
        }
        (
            Expr::Binary {
                left: left_left,
                op: left_op,
                right: left_right,
            },
            Expr::Binary {
                left: right_left,
                op: right_op,
                right: right_right,
            },
        ) => {
            std::mem::discriminant(left_op) == std::mem::discriminant(right_op)
                && left_left.structurally_eq(right_left)
                && left_right.structurally_eq(right_right)
        }
        (
            Expr::IsNull {
                expr: left,
                negated: left_negated,
            },
            Expr::IsNull {
                expr: right,
                negated: right_negated,
            },
        ) => left_negated == right_negated && left.structurally_eq(right),
        (
            Expr::InList {
                expr: left,
                values: left_values,
                negated: left_negated,
            },
            Expr::InList {
                expr: right,
                values: right_values,
                negated: right_negated,
            },
        ) => {
            left_negated == right_negated
                && left.structurally_eq(right)
                && all_structurally_eq(left_values, right_values)
        }
        (
            Expr::Between {
                expr: left,
                low: left_low,
                high: left_high,
                negated: left_negated,
            },
            Expr::Between {
                expr: right,
                low: right_low,
                high: right_high,
                negated: right_negated,
            },
        ) => {
            left_negated == right_negated
                && left.structurally_eq(right)
                && left_low.structurally_eq(right_low)
                && left_high.structurally_eq(right_high)
        }
        (Expr::Not { expr: left }, Expr::Not { expr: right }) => left.structurally_eq(right),
        (
            Expr::Cast {
                expr: left,
                data_type: left_type,
            },
            Expr::Cast {
                expr: right,
                data_type: right_type,
            },
        ) => left_type == right_type && left.structurally_eq(right),
        _ => false,
    }
}

fn optional_structurally_eq(left: Option<&Expr>, right: Option<&Expr>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left.structurally_eq(right),
        (None, None) => true,
        _ => false,
    }
}

fn all_structurally_eq(left: &[Expr], right: &[Expr]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| left.structurally_eq(right))
}
