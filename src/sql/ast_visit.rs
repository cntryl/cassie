//! Structural traversal helpers for [`Expr`].
//!
//! Expression walkers that only need to recurse into sub-expressions should
//! use these helpers instead of re-listing every variant, so adding a variant
//! (or a new child such as a CASE branch) only touches this module.

use std::ops::ControlFlow;

use super::ast_query::Expr;

impl Expr {
    /// Visits each direct sub-expression in structural order, stopping early
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
            Self::RowCompare(comparison) => {
                for field in comparison.left.iter().chain(&comparison.right) {
                    visit(field)?;
                }
            }
            Self::Binary { left, right, .. } => {
                visit(left)?;
                visit(right)?;
            }
            Self::ArraySubscript { array, subscripts } => {
                visit(array)?;
                for subscript in subscripts {
                    match subscript {
                        super::ast_query::ArraySubscript::Index(index) => visit(index)?,
                        super::ast_query::ArraySubscript::Slice { lower, upper } => {
                            if let Some(lower) = lower {
                                visit(lower)?;
                            }
                            if let Some(upper) = upper {
                                visit(upper)?;
                            }
                        }
                    }
                }
            }
            Self::IsNull { expr, .. }
            | Self::TruthTest { expr, .. }
            | Self::Not { expr }
            | Self::Cast { expr, .. }
            | Self::Collate { expr, .. }
            | Self::UnaryNumeric { expr, .. }
            | Self::IntegerNegate { expr, .. } => {
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
            }
            | Self::Like {
                expr,
                pattern: low,
                escape: high,
                ..
            } => {
                visit(expr)?;
                visit(low)?;
                visit(high)?;
            }
            Self::Function(function) => {
                for arg in function.expressions() {
                    visit(arg)?;
                }
            }
            Self::Subquery(subquery) => {
                for operand in subquery.operands() {
                    visit(operand)?;
                }
            }
            Self::Column(_)
            | Self::Param(_)
            | Self::StringLiteral(_)
            | Self::NumberLiteral(_)
            | Self::IntegerLiteral(_)
            | Self::NumericLiteral(_)
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
            } => map_case_children(operand.as_deref(), branches, else_expr.as_deref(), &mut map),
            Self::RowCompare(comparison) => Self::RowCompare(Box::new(super::RowComparison {
                left: comparison.left.iter().map(&mut map).collect(),
                op: comparison.op,
                right: comparison.right.iter().map(&mut map).collect(),
            })),
            Self::Binary { left, op, right } => Self::Binary {
                left: Box::new(map(left)),
                op: op.clone(),
                right: Box::new(map(right)),
            },
            Self::ArraySubscript { array, subscripts } => Self::ArraySubscript {
                array: Box::new(map(array)),
                subscripts: subscripts
                    .iter()
                    .map(|subscript| map_subscript(subscript, &mut map))
                    .collect(),
            },
            Self::TruthTest {
                expr,
                truth,
                negated,
            } => Self::TruthTest {
                expr: Box::new(map(expr)),
                truth: *truth,
                negated: *negated,
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
            Self::Like { .. } | Self::Between { .. } => map_predicate(self, &mut map),
            Self::Not { expr } => Self::Not {
                expr: Box::new(map(expr)),
            },
            Self::UnaryNumeric { expr, op } => Self::UnaryNumeric {
                expr: Box::new(map(expr)),
                op: *op,
            },
            Self::IntegerNegate { expr, width } => Self::IntegerNegate {
                expr: Box::new(map(expr)),
                width: *width,
            },
            Self::Cast {
                expr,
                data_type,
                source_type,
                identity,
            } => Self::Cast {
                expr: Box::new(map(expr)),
                data_type: data_type.clone(),
                source_type: source_type.clone(),
                identity: *identity,
            },
            Self::Collate { expr, collation } => Self::Collate {
                expr: Box::new(map(expr)),
                collation: collation.clone(),
            },
            Self::Function(function) => Self::Function(Box::new(super::ast_query::FunctionCall {
                name: function.name.clone(),
                args: function.args.iter().map(&mut map).collect(),
                quantifier: function.quantifier,
                filter: function.filter.as_deref().map(|expr| Box::new(map(expr))),
                order_by: function
                    .order_by
                    .iter()
                    .map(|order| super::ast_query::OrderExpr {
                        expr: map(&order.expr),
                        direction: order.direction.clone(),
                        nulls: order.nulls,
                    })
                    .collect(),
            })),
            Self::Subquery(subquery) => Self::Subquery(Box::new(subquery.map_operands(map))),
            Self::Column(_)
            | Self::Param(_)
            | Self::StringLiteral(_)
            | Self::NumberLiteral(_)
            | Self::IntegerLiteral(_)
            | Self::NumericLiteral(_)
            | Self::BoolLiteral(_)
            | Self::Null
            | Self::Exists(_) => self.clone(),
        }
    }

    /// Visits each direct sub-expression in structural order.
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

    /// Mutates each direct sub-expression in structural order.
    ///
    /// # Errors
    ///
    /// Returns the first error produced by `visit`.
    pub fn try_visit_children_mut<E>(
        &mut self,
        mut visit: impl FnMut(&mut Expr) -> Result<(), E>,
    ) -> Result<(), E> {
        match self {
            Self::RowCompare(comparison) => {
                for field in comparison.left.iter_mut().chain(&mut comparison.right) {
                    visit(field)?;
                }
            }
            Self::Binary { left, right, .. } => {
                visit(left)?;
                visit(right)?;
            }
            Self::ArraySubscript { array, subscripts } => {
                visit(array)?;
                for subscript in subscripts {
                    match subscript {
                        super::ast_query::ArraySubscript::Index(index) => visit(index)?,
                        super::ast_query::ArraySubscript::Slice { lower, upper } => {
                            if let Some(lower) = lower {
                                visit(lower)?;
                            }
                            if let Some(upper) = upper {
                                visit(upper)?;
                            }
                        }
                    }
                }
            }
            Self::Cast { expr, .. }
            | Self::Collate { expr, .. }
            | Self::UnaryNumeric { expr, .. }
            | Self::IntegerNegate { expr, .. }
            | Self::Not { expr }
            | Self::IsNull { expr, .. }
            | Self::TruthTest { expr, .. } => {
                visit(expr)?;
            }
            Self::Between {
                expr, low, high, ..
            }
            | Self::Like {
                expr,
                pattern: low,
                escape: high,
                ..
            } => {
                visit(expr)?;
                visit(low)?;
                visit(high)?;
            }
            Self::InList { expr, values, .. } => {
                visit(expr)?;
                for value in values {
                    visit(value)?;
                }
            }
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
                if let Some(otherwise) = else_expr {
                    visit(otherwise)?;
                }
            }
            Self::Function(function) => {
                for argument in function.expressions_mut() {
                    visit(argument)?;
                }
            }
            Self::Subquery(subquery) => {
                for operand in subquery.operands_mut() {
                    visit(operand)?;
                }
            }
            Self::Column(_)
            | Self::Param(_)
            | Self::StringLiteral(_)
            | Self::NumberLiteral(_)
            | Self::IntegerLiteral(_)
            | Self::NumericLiteral(_)
            | Self::BoolLiteral(_)
            | Self::Null
            | Self::Exists(_) => {}
        }
        Ok(())
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

    /// Compares two expressions structurally, retaining identifier case for
    /// columns while ignoring ASCII case in function names.
    #[must_use]
    pub fn structurally_eq(&self, other: &Expr) -> bool {
        match (self, other) {
            (Self::Column(left), Self::Column(right))
            | (Self::NumericLiteral(left), Self::NumericLiteral(right))
            | (Self::StringLiteral(left), Self::StringLiteral(right)) => left == right,
            (Self::Param(left), Self::Param(right)) => left == right,
            (Self::NumberLiteral(left), Self::NumberLiteral(right)) => {
                left.to_bits() == right.to_bits()
            }
            (Self::IntegerLiteral(left), Self::IntegerLiteral(right)) => left == right,
            (
                Self::Collate {
                    expr: left_expr,
                    collation: left_collation,
                },
                Self::Collate {
                    expr: right_expr,
                    collation: right_collation,
                },
            ) => left_collation == right_collation && left_expr.structurally_eq(right_expr),
            (Self::BoolLiteral(left), Self::BoolLiteral(right)) => left == right,
            (Self::Null, Self::Null) => true,
            (Self::RowCompare(left), Self::RowCompare(right)) => {
                left.op == right.op
                    && all_structurally_eq(&left.left, &right.left)
                    && all_structurally_eq(&left.right, &right.right)
            }
            (Self::Function(left), Self::Function(right)) => {
                left.name.eq_ignore_ascii_case(&right.name)
                    && all_structurally_eq(&left.args, &right.args)
                    && left.is_distinct() == right.is_distinct()
                    && optional_structurally_eq(left.filter.as_deref(), right.filter.as_deref())
                    && left.order_by.len() == right.order_by.len()
                    && left
                        .order_by
                        .iter()
                        .zip(&right.order_by)
                        .all(|(left, right)| {
                            left.expr.structurally_eq(&right.expr)
                                && std::mem::discriminant(&left.direction)
                                    == std::mem::discriminant(&right.direction)
                                && left.nulls == right.nulls
                        })
            }
            (Self::Exists(left), Self::Exists(right)) => {
                format!("{left:?}") == format!("{right:?}")
            }
            (Self::Subquery(left), Self::Subquery(right)) => left.structurally_eq(right),
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
    if let Some(equal) = like_structurally_eq(left, right) {
        return equal;
    }
    if let Some(equal) = truth_test_eq(left, right) {
        return equal;
    }

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
        ) => binary_structurally_eq(
            (left_left, left_op, left_right),
            (right_left, right_op, right_right),
        ),
        (
            Expr::ArraySubscript {
                array: left,
                subscripts: left_subscripts,
            },
            Expr::ArraySubscript {
                array: right,
                subscripts: right_subscripts,
            },
        ) => {
            left.structurally_eq(right)
                && subscripts_structurally_eq(left_subscripts, right_subscripts)
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
        (Expr::Between { .. }, Expr::Between { .. }) => between_structurally_eq(left, right),
        _ => unary_structurally_eq(left, right),
    }
}

fn between_structurally_eq(left: &Expr, right: &Expr) -> bool {
    match (left, right) {
        (
            Expr::Between {
                expr: left,
                low: left_low,
                high: left_high,
                negated: left_negated,
                symmetric: left_symmetric,
            },
            Expr::Between {
                expr: right,
                low: right_low,
                high: right_high,
                negated: right_negated,
                symmetric: right_symmetric,
            },
        ) => {
            left_negated == right_negated
                && left_symmetric == right_symmetric
                && left.structurally_eq(right)
                && left_low.structurally_eq(right_low)
                && left_high.structurally_eq(right_high)
        }
        _ => false,
    }
}

fn binary_structurally_eq(
    left: (&Expr, &super::BinaryOp, &Expr),
    right: (&Expr, &super::BinaryOp, &Expr),
) -> bool {
    left.1 == right.1 && left.0.structurally_eq(right.0) && left.2.structurally_eq(right.2)
}

fn map_subscript(
    subscript: &super::ast_query::ArraySubscript,
    map: &mut impl FnMut(&Expr) -> Expr,
) -> super::ast_query::ArraySubscript {
    match subscript {
        super::ast_query::ArraySubscript::Index(index) => {
            super::ast_query::ArraySubscript::Index(map(index))
        }
        super::ast_query::ArraySubscript::Slice { lower, upper } => {
            super::ast_query::ArraySubscript::Slice {
                lower: lower.as_ref().map(&mut *map),
                upper: upper.as_ref().map(map),
            }
        }
    }
}

fn subscripts_structurally_eq(
    left: &[super::ast_query::ArraySubscript],
    right: &[super::ast_query::ArraySubscript],
) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| match (left, right) {
                (
                    super::ast_query::ArraySubscript::Index(left),
                    super::ast_query::ArraySubscript::Index(right),
                ) => left.structurally_eq(right),
                (
                    super::ast_query::ArraySubscript::Slice {
                        lower: left_lower,
                        upper: left_upper,
                    },
                    super::ast_query::ArraySubscript::Slice {
                        lower: right_lower,
                        upper: right_upper,
                    },
                ) => {
                    optional_structurally_eq(left_lower.as_ref(), right_lower.as_ref())
                        && optional_structurally_eq(left_upper.as_ref(), right_upper.as_ref())
                }
                _ => false,
            })
}

fn unary_structurally_eq(left: &Expr, right: &Expr) -> bool {
    match (left, right) {
        (Expr::Not { expr: left }, Expr::Not { expr: right }) => left.structurally_eq(right),
        (
            Expr::Cast {
                expr: left,
                data_type: left_type,
                source_type: left_source,
                identity: left_identity,
            },
            Expr::Cast {
                expr: right,
                data_type: right_type,
                source_type: right_source,
                identity: right_identity,
            },
        ) => {
            left_type == right_type
                && left_source == right_source
                && left_identity == right_identity
                && left.structurally_eq(right)
        }
        (
            Expr::Collate {
                expr: left,
                collation: left_collation,
            },
            Expr::Collate {
                expr: right,
                collation: right_collation,
            },
        ) => left_collation == right_collation && left.structurally_eq(right),
        (
            Expr::UnaryNumeric {
                expr: left,
                op: lop,
            },
            Expr::UnaryNumeric {
                expr: right,
                op: rop,
            },
        ) => lop == rop && left.structurally_eq(right),
        (
            Expr::IntegerNegate {
                expr: left,
                width: lw,
            },
            Expr::IntegerNegate {
                expr: right,
                width: rw,
            },
        ) => lw == rw && left.structurally_eq(right),
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

fn map_predicate(expr: &Expr, mut map: impl FnMut(&Expr) -> Expr) -> Expr {
    match expr {
        Expr::Like {
            expr,
            pattern,
            escape,
            case_insensitive,
        } => Expr::Like {
            expr: Box::new(map(expr)),
            pattern: Box::new(map(pattern)),
            escape: Box::new(map(escape)),
            case_insensitive: *case_insensitive,
        },
        Expr::Between {
            expr,
            low,
            high,
            negated,
            symmetric,
        } => Expr::Between {
            expr: Box::new(map(expr)),
            low: Box::new(map(low)),
            high: Box::new(map(high)),
            negated: *negated,
            symmetric: *symmetric,
        },
        _ => unreachable!("ternary predicate mapping"),
    }
}

fn like_structurally_eq(left: &Expr, right: &Expr) -> Option<bool> {
    match (left, right) {
        (
            Expr::Like {
                expr: left,
                pattern: lp,
                escape: le,
                case_insensitive: left_case_insensitive,
            },
            Expr::Like {
                expr: right,
                pattern: rp,
                escape: re,
                case_insensitive: right_case_insensitive,
            },
        ) => Some(
            left.structurally_eq(right)
                && lp.structurally_eq(rp)
                && le.structurally_eq(re)
                && left_case_insensitive == right_case_insensitive,
        ),

        (Expr::Like { .. }, _) | (_, Expr::Like { .. }) => Some(false),
        _ => None,
    }
}

#[cfg(test)]
#[path = "ast_visit_tests.rs"]
mod tests;

fn map_case_children(
    operand: Option<&Expr>,
    branches: &[(Expr, Expr)],
    else_expr: Option<&Expr>,
    map: &mut impl FnMut(&Expr) -> Expr,
) -> Expr {
    Expr::Case {
        operand: operand.map(|expr| Box::new(map(expr))),
        branches: branches
            .iter()
            .map(|(when, then)| (map(when), map(then)))
            .collect(),
        else_expr: else_expr.map(|expr| Box::new(map(expr))),
    }
}

fn truth_test_eq(left: &Expr, right: &Expr) -> Option<bool> {
    let (
        Expr::TruthTest {
            expr: left,
            truth: left_truth,
            negated: left_negated,
        },
        Expr::TruthTest {
            expr: right,
            truth: right_truth,
            negated: right_negated,
        },
    ) = (left, right)
    else {
        return None;
    };
    Some(left_truth == right_truth && left_negated == right_negated && left.structurally_eq(right))
}
