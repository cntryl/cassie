//! Scalar, row, `IN`, `ANY` and `ALL` subquery expressions (PostgreSQL sublinks).
//!
//! `EXISTS` keeps its own [`Expr::Exists`] node. Every other sublink is one
//! [`SubqueryExpr`]: the subquery statement, the outer operands it compares
//! against and, once executed, its bounded result rows.
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use super::{BinaryOp, Expr, ParsedStatement, RowComparisonOp};
use crate::types::{DataType, Value};

/// PostgreSQL's message (SQLSTATE 21000) for a multi-row scalar subquery.
pub const SUBQUERY_MULTIPLE_ROWS: &str =
    "more than one row returned by a subquery used as an expression";

/// How a subquery's rows become one expression value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SubqueryKind {
    /// `(SELECT x ...)`: zero rows yield NULL, one row its value and more
    /// rows a cardinality violation.
    Scalar,
    /// `ARRAY(SELECT x ...)`: every row's value, in row order.
    Array,
    /// `(a, b) op (SELECT x, y ...)`: a one-row subquery compared as a row.
    Row {
        left: Vec<Expr>,
        op: RowComparisonOp,
    },
    /// `left op ANY|ALL (SELECT ...)`; `IN` is `= ANY` and `NOT IN` is
    /// `NOT (= ANY)`. A row constructor on the left has several operands.
    Quantified {
        left: Vec<Expr>,
        op: BinaryOp,
        all: bool,
        /// Spelled `[NOT] IN`, retained for result naming and display.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        membership: bool,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        negated: bool,
    },
}

/// A sublink and, after execution, its outcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubqueryExpr {
    pub kind: SubqueryKind,
    pub query: Box<ParsedStatement>,
    /// The bound type of the first output column (scalar/array element).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_type: Option<DataType>,
    /// The executed rows; never part of a cached or serialized plan.
    #[serde(skip)]
    pub outcome: Option<SubqueryOutcome>,
}

/// The rows an executed subquery produced, or the execution error that is
/// raised only when the expression is actually evaluated (PostgreSQL runs
/// an uncorrelated subquery when its value is first needed, so an
/// unselected CASE branch never reports its error).
#[derive(Debug, Clone)]
pub enum SubqueryOutcome {
    Rows(Arc<SubqueryRows>),
    Failed(Arc<Mutex<Option<crate::app::CassieError>>>),
}

/// Executed subquery rows and the query memory admitted for them.
#[derive(Debug)]
pub struct SubqueryRows {
    pub rows: Vec<Vec<Value>>,
    pub memory: Option<crate::runtime::QueryMemoryReservation>,
}

impl SubqueryOutcome {
    /// Wraps an execution failure for later evaluation.
    #[must_use]
    pub fn failed(error: crate::app::CassieError) -> Self {
        Self::Failed(Arc::new(Mutex::new(Some(error))))
    }
}

impl SubqueryExpr {
    #[must_use]
    pub fn new(kind: SubqueryKind, query: ParsedStatement) -> Self {
        Self {
            kind,
            query: Box::new(query),
            data_type: None,
            outcome: None,
        }
    }

    /// The outer operands this sublink compares against.
    #[must_use]
    pub fn operands(&self) -> &[Expr] {
        match &self.kind {
            SubqueryKind::Scalar | SubqueryKind::Array => &[],
            SubqueryKind::Row { left, .. } | SubqueryKind::Quantified { left, .. } => left,
        }
    }

    /// Mutable outer operands, in source order.
    pub fn operands_mut(&mut self) -> &mut [Expr] {
        match &mut self.kind {
            SubqueryKind::Scalar | SubqueryKind::Array => &mut [],
            SubqueryKind::Row { left, .. } | SubqueryKind::Quantified { left, .. } => left,
        }
    }

    /// Returns whether the sublink yields a boolean predicate.
    #[must_use]
    pub const fn is_predicate(&self) -> bool {
        matches!(
            self.kind,
            SubqueryKind::Row { .. } | SubqueryKind::Quantified { .. }
        )
    }

    /// The number of columns the subquery must return, if fixed.
    #[must_use]
    pub fn required_width(&self) -> usize {
        match &self.kind {
            SubqueryKind::Scalar | SubqueryKind::Array => 1,
            SubqueryKind::Row { left, .. } | SubqueryKind::Quantified { left, .. } => left.len(),
        }
    }

    /// The PostgreSQL 42601 error for a subquery returning `actual` columns.
    #[must_use]
    pub fn width_error(&self, actual: usize) -> crate::app::CassieError {
        let message = match &self.kind {
            SubqueryKind::Scalar | SubqueryKind::Array => "subquery must return only one column",
            SubqueryKind::Row { left, .. } | SubqueryKind::Quantified { left, .. } => {
                if actual > left.len() {
                    "subquery has too many columns"
                } else {
                    "subquery has too few columns"
                }
            }
        };
        crate::app::CassieError::Diagnostic(Box::new(crate::app::DiagnosticError::new(
            "42601", message,
        )))
    }

    /// The result type this sublink contributes to its expression.
    #[must_use]
    pub fn result_type(&self) -> Option<DataType> {
        match &self.kind {
            SubqueryKind::Scalar => self.data_type.clone(),
            SubqueryKind::Array => self
                .data_type
                .clone()
                .map(|element| DataType::Array(Box::new(element))),
            SubqueryKind::Row { .. } | SubqueryKind::Quantified { .. } => Some(DataType::Boolean),
        }
    }

    /// Rebuilds the sublink with every outer operand replaced by `map`.
    #[must_use]
    pub fn map_operands(&self, mut map: impl FnMut(&Expr) -> Expr) -> Self {
        let mut mapped = self.clone();
        for operand in mapped.operands_mut() {
            *operand = map(operand);
        }
        mapped
    }

    /// Compares two sublinks by their statement and operands.
    #[must_use]
    pub fn structurally_eq(&self, other: &Self) -> bool {
        format!("{:?}{:?}", self.kind, self.query) == format!("{:?}{:?}", other.kind, other.query)
    }
}
