use super::{BinaryOp, Cassie, Expr, Value};
use crate::catalog::IndexMeta;
use crate::types::semantic::compare_values;
use crate::types::DataType;
use std::cmp::Ordering;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default)]
pub(super) struct ConcreteConstraint {
    pub(super) equality: Option<serde_json::Value>,
    pub(super) lower: Option<ConcreteBound>,
    pub(super) upper: Option<ConcreteBound>,
    pub(super) unsatisfiable: bool,
}

#[derive(Debug, Clone)]
pub(super) struct ConcreteBound {
    pub(super) value: serde_json::Value,
    pub(super) inclusive: bool,
}

pub(super) fn canonicalize_field_constraints(
    cassie: &Cassie,
    collection: &str,
    constraints: &mut BTreeMap<String, ConcreteConstraint>,
) {
    for (field, constraint) in constraints {
        let canonicalize: fn(&mut serde_json::Value) =
            match cassie.catalog.field_type(collection, field) {
                Some(DataType::Float) => canonicalize_float_number,
                Some(DataType::Timestamp) => canonicalize_timestamp_text,
                Some(DataType::SmallInt | DataType::Int | DataType::BigInt) => {
                    canonicalize_integer_constraint(constraint);
                    refresh_constraint_satisfiability(constraint);
                    continue;
                }
                _ => continue,
            };
        if let Some(value) = constraint.equality.as_mut() {
            canonicalize(value);
        }
        if let Some(bound) = constraint.lower.as_mut() {
            canonicalize(&mut bound.value);
        }
        if let Some(bound) = constraint.upper.as_mut() {
            canonicalize(&mut bound.value);
        }
        refresh_constraint_satisfiability(constraint);
    }
}

/// Returns true when an equality probe on the first key field of `index`
/// reaches every row with that value. Rows whose later key fields are NULL are
/// not indexed, so those fields must be NOT NULL.
pub(in crate::executor::execution) fn index_trailing_keys_not_null(
    cassie: &Cassie,
    collection: &str,
    index: &IndexMeta,
) -> bool {
    if !index.normalized_expressions().is_empty() {
        return false;
    }
    let fields = index.normalized_fields();
    if fields.len() <= 1 {
        return true;
    }
    let not_null_fields = cassie.catalog.not_null_fields(collection);
    fields.iter().skip(1).all(|field| {
        not_null_fields.contains(&crate::sql::ColumnIdentifierPath::stored_field_key(field))
    })
}

/// Widens a canonical-shaped timestamp probe (`...SSZ`) to the fixed-width
/// form stored keys use. This is the same text normalization the filter
/// operator applies, so index probes and scans agree on every literal.
fn canonicalize_timestamp_text(value: &mut serde_json::Value) {
    let serde_json::Value::String(text) = value else {
        return;
    };
    if let std::borrow::Cow::Owned(widened) = crate::types::temporal::timestamp_order_text(text) {
        *text = widened;
    }
}

pub(super) fn canonicalize_float_number(value: &mut serde_json::Value) {
    let serde_json::Value::Number(number) = value else {
        return;
    };
    let Some(number) = number.as_f64().and_then(serde_json::Number::from_f64) else {
        return;
    };
    *value = serde_json::Value::Number(number);
}

/// How a float-shaped probe or bound on an integer-typed column rewrites.
#[derive(Debug, Clone, Copy)]
enum IntegerRewrite {
    /// The integer-shaped value the bound narrows to.
    Value(i64),
    /// Every stored integer satisfies the bound, so it can be dropped.
    Unbounded,
    /// No stored integer can satisfy the bound.
    Unsatisfiable,
    /// Already integer-shaped, so the stored key shape already matches.
    Unchanged,
}

/// Narrows a float-shaped constraint on an integer-typed column to the integer
/// shape scalar-index keys use.
///
/// `append_scalar_value` tags integer keys `0x30` and float keys `0x40`, so
/// every stored integer key sorts below any float-shaped bound: without this,
/// an index range scan returns no rows for `n > 5.5` and every row for
/// `n < 5.5`, while a full scan of the same table returns the correct rows.
/// Narrowing is exact over the integers (`n > 5.5` is `n >= 6`, `n < 5.5` is
/// `n <= 5`), so the bounds stay exact and the residual filter can still be
/// skipped.
fn canonicalize_integer_constraint(constraint: &mut ConcreteConstraint) {
    if let Some(equality) = constraint.equality.as_mut() {
        match integer_equality(equality) {
            IntegerRewrite::Value(value) => *equality = serde_json::Value::Number(value.into()),
            IntegerRewrite::Unsatisfiable => constraint.unsatisfiable = true,
            IntegerRewrite::Unbounded | IntegerRewrite::Unchanged => {}
        }
    }
    narrow_integer_bound(
        &mut constraint.lower,
        &mut constraint.unsatisfiable,
        integer_lower_bound,
    );
    narrow_integer_bound(
        &mut constraint.upper,
        &mut constraint.unsatisfiable,
        integer_upper_bound,
    );
}

fn narrow_integer_bound(
    bound: &mut Option<ConcreteBound>,
    unsatisfiable: &mut bool,
    rewrite: fn(&ConcreteBound) -> IntegerRewrite,
) {
    let Some(current) = bound.as_ref() else {
        return;
    };
    match rewrite(current) {
        IntegerRewrite::Value(value) => {
            *bound = Some(ConcreteBound {
                value: serde_json::Value::Number(value.into()),
                inclusive: true,
            });
        }
        IntegerRewrite::Unbounded => *bound = None,
        IntegerRewrite::Unsatisfiable => *unsatisfiable = true,
        IntegerRewrite::Unchanged => {}
    }
}

/// Returns the finite float a JSON number carries when it is float-shaped.
/// Integer-shaped numbers already match the stored key shape.
fn float_shaped_number(value: &serde_json::Value) -> Option<f64> {
    let serde_json::Value::Number(number) = value else {
        return None;
    };
    if number.as_i64().is_some() || number.as_u64().is_some() {
        return None;
    }
    number.as_f64().filter(|float| float.is_finite())
}

fn integral_to_i64(value: f64) -> Option<i64> {
    format!("{value:.0}").parse::<i64>().ok()
}

fn integer_equality(value: &serde_json::Value) -> IntegerRewrite {
    let Some(float) = float_shaped_number(value) else {
        return IntegerRewrite::Unchanged;
    };
    if float.fract() != 0.0 {
        return IntegerRewrite::Unsatisfiable;
    }
    integral_to_i64(float).map_or(IntegerRewrite::Unsatisfiable, IntegerRewrite::Value)
}

fn integer_lower_bound(bound: &ConcreteBound) -> IntegerRewrite {
    let Some(float) = float_shaped_number(&bound.value) else {
        return IntegerRewrite::Unchanged;
    };
    if !bound.inclusive && float.fract() == 0.0 {
        return integral_to_i64(float).map_or_else(
            || {
                if float.is_sign_positive() {
                    IntegerRewrite::Unsatisfiable
                } else {
                    IntegerRewrite::Unbounded
                }
            },
            |value| {
                value
                    .checked_add(1)
                    .map_or(IntegerRewrite::Unsatisfiable, IntegerRewrite::Value)
            },
        );
    }
    let smallest = if bound.inclusive {
        float.ceil()
    } else {
        float.floor() + 1.0
    };
    integral_to_i64(smallest).map_or_else(
        || {
            if smallest.is_sign_positive() {
                IntegerRewrite::Unsatisfiable
            } else {
                IntegerRewrite::Unbounded
            }
        },
        IntegerRewrite::Value,
    )
}

fn integer_upper_bound(bound: &ConcreteBound) -> IntegerRewrite {
    let Some(float) = float_shaped_number(&bound.value) else {
        return IntegerRewrite::Unchanged;
    };
    if !bound.inclusive && float.fract() == 0.0 {
        return integral_to_i64(float).map_or_else(
            || {
                if float.is_sign_positive() {
                    IntegerRewrite::Unbounded
                } else {
                    IntegerRewrite::Unsatisfiable
                }
            },
            |value| {
                value
                    .checked_sub(1)
                    .map_or(IntegerRewrite::Unsatisfiable, IntegerRewrite::Value)
            },
        );
    }
    let largest = if bound.inclusive {
        float.floor()
    } else {
        float.ceil() - 1.0
    };
    integral_to_i64(largest).map_or_else(
        || {
            if largest.is_sign_positive() {
                IntegerRewrite::Unbounded
            } else {
                IntegerRewrite::Unsatisfiable
            }
        },
        IntegerRewrite::Value,
    )
}

fn intersect_lower_bound(
    bound: &mut Option<ConcreteBound>,
    value: serde_json::Value,
    inclusive: bool,
) {
    intersect_bound(bound, ConcreteBound { value, inclusive }, Ordering::Greater);
}

fn intersect_upper_bound(
    bound: &mut Option<ConcreteBound>,
    value: serde_json::Value,
    inclusive: bool,
) {
    intersect_bound(bound, ConcreteBound { value, inclusive }, Ordering::Less);
}

fn intersect_constraint(
    constraint: &mut ConcreteConstraint,
    op: &BinaryOp,
    value: serde_json::Value,
) -> Option<()> {
    // Comparisons with NULL never match, so a NULL bound must not widen the scan.
    let compares_with_null = value.is_null();
    match op {
        BinaryOp::Eq => {
            if constraint
                .equality
                .as_ref()
                .is_some_and(|current| compare_json_values(current, &value) != Ordering::Equal)
            {
                constraint.unsatisfiable = true;
            } else if constraint.equality.is_none() {
                constraint.equality = Some(value);
            }
        }
        BinaryOp::Gt => intersect_lower_bound(&mut constraint.lower, value, false),
        BinaryOp::Gte => intersect_lower_bound(&mut constraint.lower, value, true),
        BinaryOp::Lt => intersect_upper_bound(&mut constraint.upper, value, false),
        BinaryOp::Lte => intersect_upper_bound(&mut constraint.upper, value, true),
        _ => return None,
    }
    constraint.unsatisfiable |= compares_with_null;
    refresh_constraint_satisfiability(constraint);
    Some(())
}

fn refresh_constraint_satisfiability(constraint: &mut ConcreteConstraint) {
    constraint.unsatisfiable |= constraint
        .equality
        .as_ref()
        .is_some_and(|equality| !equality_satisfies_bounds(equality, constraint));
    constraint.unsatisfiable |= match (&constraint.lower, &constraint.upper) {
        (Some(lower), Some(upper)) => match compare_json_values(&lower.value, &upper.value) {
            Ordering::Greater => true,
            Ordering::Equal => !lower.inclusive || !upper.inclusive,
            Ordering::Less => false,
        },
        _ => false,
    };
}

fn equality_satisfies_bounds(
    equality: &serde_json::Value,
    constraint: &ConcreteConstraint,
) -> bool {
    let satisfies_lower = constraint.lower.as_ref().is_none_or(|lower| {
        let ordering = compare_json_values(equality, &lower.value);
        ordering == Ordering::Greater || ordering == Ordering::Equal && lower.inclusive
    });
    let satisfies_upper = constraint.upper.as_ref().is_none_or(|upper| {
        let ordering = compare_json_values(equality, &upper.value);
        ordering == Ordering::Less || ordering == Ordering::Equal && upper.inclusive
    });
    satisfies_lower && satisfies_upper
}

fn intersect_bound(
    bound: &mut Option<ConcreteBound>,
    candidate: ConcreteBound,
    tighter_ordering: Ordering,
) {
    let replace = bound.as_ref().is_none_or(|current| {
        let ordering = compare_json_values(&candidate.value, &current.value);
        ordering == tighter_ordering
            || ordering == Ordering::Equal && !candidate.inclusive && current.inclusive
    });
    if replace {
        *bound = Some(candidate);
    }
}

fn compare_json_values(left: &serde_json::Value, right: &serde_json::Value) -> Ordering {
    compare_values(&concrete_bound_value(left), &concrete_bound_value(right))
}

fn concrete_bound_value(value: &serde_json::Value) -> Value {
    match value {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(value) => Value::Bool(*value),
        serde_json::Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                Value::Int64(value)
            } else if let Some(value) = value.as_f64() {
                Value::Float64(value)
            } else {
                Value::Json(serde_json::Value::Number(value.clone()))
            }
        }
        serde_json::Value::String(value) => Value::String(value.clone()),
        value => Value::Json(value.clone()),
    }
}

pub(super) fn concrete_constraints(
    filter: Option<&Expr>,
    params: &[Value],
) -> Option<BTreeMap<String, ConcreteConstraint>> {
    let mut constraints = BTreeMap::new();
    let Some(filter) = filter else {
        return Some(constraints);
    };
    collect_concrete_constraints(filter, params, &mut constraints)?;
    Some(constraints)
}

#[derive(Default)]
pub(super) struct ExpressionIndexConstraints {
    pub(super) fields: BTreeMap<String, ConcreteConstraint>,
    pub(super) expressions: BTreeMap<String, ConcreteConstraint>,
}

pub(super) fn expression_index_constraints(
    filter: Option<&Expr>,
    params: &[Value],
) -> Option<ExpressionIndexConstraints> {
    let mut constraints = ExpressionIndexConstraints::default();
    let Some(filter) = filter else {
        return Some(constraints);
    };
    collect_expression_index_constraints(filter, params, &mut constraints)?;
    Some(constraints)
}

fn collect_expression_index_constraints(
    expr: &Expr,
    params: &[Value],
    constraints: &mut ExpressionIndexConstraints,
) -> Option<()> {
    match expr {
        Expr::Binary {
            left,
            op: BinaryOp::And,
            right,
        } => {
            collect_expression_index_constraints(left, params, constraints)?;
            collect_expression_index_constraints(right, params, constraints)
        }
        Expr::Binary { left, op, right } => {
            if let Some((field, op, value)) = concrete_constraint(left, op, right, params) {
                return intersect_constraint(
                    constraints.fields.entry(field).or_default(),
                    &op,
                    value,
                );
            }
            let (expression, op, value) = concrete_expression_constraint(left, op, right, params)?;
            intersect_constraint(
                constraints.expressions.entry(expression).or_default(),
                &op,
                value,
            )
        }
        Expr::Between {
            expr,
            low,
            high,
            negated: false,
        } if expr_has_column(expr) && !matches!(expr.as_ref(), Expr::Column(_)) => {
            let constraint = constraints
                .expressions
                .entry(serde_json::to_string(expr.as_ref()).ok()?)
                .or_default();
            intersect_constraint(constraint, &BinaryOp::Gte, expr_to_json(low, params)?)?;
            intersect_constraint(constraint, &BinaryOp::Lte, expr_to_json(high, params)?)
        }
        _ => None,
    }
}

fn concrete_expression_constraint(
    left: &Expr,
    op: &BinaryOp,
    right: &Expr,
    params: &[Value],
) -> Option<(String, BinaryOp, serde_json::Value)> {
    match (left, right) {
        (expr, value) if expr_has_column(expr) && !matches!(expr, Expr::Column(_)) => Some((
            serde_json::to_string(expr).ok()?,
            op.clone(),
            expr_to_json(value, params)?,
        )),
        (value, expr) if expr_has_column(expr) && !matches!(expr, Expr::Column(_)) => Some((
            serde_json::to_string(expr).ok()?,
            reverse_binary_op(op)?,
            expr_to_json(value, params)?,
        )),
        _ => None,
    }
}

fn expr_has_column(expr: &Expr) -> bool {
    matches!(expr, Expr::Column(_)) || expr.any_child(expr_has_column)
}

fn collect_concrete_constraints(
    expr: &Expr,
    params: &[Value],
    constraints: &mut BTreeMap<String, ConcreteConstraint>,
) -> Option<()> {
    match expr {
        Expr::Binary {
            left,
            op: BinaryOp::And,
            right,
        } => {
            collect_concrete_constraints(left, params, constraints)?;
            collect_concrete_constraints(right, params, constraints)
        }
        Expr::Binary { left, op, right } => {
            let (field, op, value) = concrete_constraint(left, op, right, params)?;
            let entry = constraints.entry(field).or_default();
            intersect_constraint(entry, &op, value)
        }
        Expr::Between {
            expr,
            low,
            high,
            negated: false,
        } => {
            let Expr::Column(field) = expr.as_ref() else {
                return None;
            };
            let entry = constraints
                .entry(crate::sql::ColumnIdentifierPath::reference_field_key(field))
                .or_default();
            intersect_constraint(entry, &BinaryOp::Gte, expr_to_json(low, params)?)?;
            intersect_constraint(entry, &BinaryOp::Lte, expr_to_json(high, params)?)
        }
        _ => None,
    }
}

fn concrete_constraint(
    left: &Expr,
    op: &BinaryOp,
    right: &Expr,
    params: &[Value],
) -> Option<(String, BinaryOp, serde_json::Value)> {
    match (left, right) {
        (Expr::Column(field), other) => Some((
            crate::sql::ColumnIdentifierPath::reference_field_key(field),
            op.clone(),
            expr_to_json(other, params)?,
        )),
        (other, Expr::Column(field)) => Some((
            crate::sql::ColumnIdentifierPath::reference_field_key(field),
            reverse_binary_op(op)?,
            expr_to_json(other, params)?,
        )),
        _ => None,
    }
}

fn reverse_binary_op(op: &BinaryOp) -> Option<BinaryOp> {
    match op {
        BinaryOp::Eq => Some(BinaryOp::Eq),
        BinaryOp::Gt => Some(BinaryOp::Lt),
        BinaryOp::Gte => Some(BinaryOp::Lte),
        BinaryOp::Lt => Some(BinaryOp::Gt),
        BinaryOp::Lte => Some(BinaryOp::Gte),
        _ => None,
    }
}

pub(super) fn expr_to_json(expr: &Expr, params: &[Value]) -> Option<serde_json::Value> {
    match expr {
        Expr::StringLiteral(value) => Some(serde_json::Value::String(value.clone())),
        Expr::NumberLiteral(value) => {
            if !value.is_finite() {
                return None;
            }
            if value.fract() == 0.0 {
                if let Ok(integer) = value.to_string().parse::<i64>() {
                    return Some(serde_json::Value::Number(integer.into()));
                }
            }
            serde_json::Number::from_f64(*value).map(serde_json::Value::Number)
        }
        Expr::IntegerLiteral(value) => Some(serde_json::Value::Number((*value).into())),
        Expr::BoolLiteral(value) => Some(serde_json::Value::Bool(*value)),
        Expr::Null => Some(serde_json::Value::Null),
        Expr::Param(index) => params.get(*index).and_then(value_to_json),
        _ => None,
    }
}

fn value_to_json(value: &Value) -> Option<serde_json::Value> {
    Some(match value {
        Value::Null => serde_json::Value::Null,
        Value::Bool(value) => serde_json::Value::Bool(*value),
        Value::Int64(value) => serde_json::Value::Number((*value).into()),
        Value::Float64(value) => serde_json::Value::Number(serde_json::Number::from_f64(*value)?),
        Value::String(value) => serde_json::Value::String(value.clone()),
        Value::Vector(value) => value
            .values
            .iter()
            .map(|value| serde_json::Number::from_f64(f64::from(*value)))
            .map(|number| number.map(serde_json::Value::Number))
            .collect::<Option<_>>()
            .map(serde_json::Value::Array)?,
        Value::Json(value) => value.clone(),
    })
}
