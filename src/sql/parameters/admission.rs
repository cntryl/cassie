//! Transient admission for nonempty wire Parse parameter metadata.
//!
//! The borrowed walk inspects nodes and parameter indices, and reads Boolean
//! comparison literal text without copying it. Invalid typed Boolean input is
//! copied only after query-memory admission. Every frame/child and every capacity
//! growth checks interruption. The selected SELECT analysis additionally owns
//! admitted occurrence, phase, target and summary storage. The explicitly
//! nonselected legacy statement visitor and its other allocations remain outside
//! that finite fold qualification.

use std::mem::size_of;

use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::sql::{ast, ParsedStatement, QueryStatement};

// PostgreSQL 18.6 MaxAllocSize (0x3fff_ffff) / sizeof(Oid) (4).
pub(super) const MAX_PARAMETER_COUNT: usize = 268_435_455;

#[derive(Debug)]
pub(crate) enum ParameterAdmissionError {
    Memory(String),
    InvalidParam(String),
    // Drop the copied text before releasing the lease that accounts for it.
    InvalidBooleanLiteral {
        input: String,
        _reservation: QueryMemoryReservation,
    },
    NonAggregateModifier {
        message: String,
        _reservation: QueryMemoryReservation,
    },
    Cancelled,
    Deadline,
    LegacyConflict(String),
    Semantic(super::diagnostics::ParameterSemanticDiagnostic),
}

#[derive(Debug)]
pub(crate) struct AdmittedParameterTypes {
    // Drop the value before releasing its transient capacity reservation.
    oids: Vec<i32>,
    _reservation: QueryMemoryReservation,
    controls: QueryExecutionControls,
}

impl AdmittedParameterTypes {
    pub(super) fn new(
        oids: Vec<i32>,
        reservation: QueryMemoryReservation,
        controls: &QueryExecutionControls,
    ) -> Self {
        Self {
            oids,
            _reservation: reservation,
            controls: controls.clone(),
        }
    }

    pub(crate) fn types(&self) -> &[i32] {
        &self.oids
    }

    pub(crate) fn final_check(&self) -> Result<(), ParameterAdmissionError> {
        check_controls(&self.controls)
    }

    // This releases transient admission. The returned legacy Vec has no lease
    // for retained prepared-statement memory, which remains a separate owner.
    pub(crate) fn into_vec(self) -> Result<Vec<i32>, ParameterAdmissionError> {
        self.final_check()?;
        Ok(self.oids)
    }
}

pub(super) fn check_controls(
    controls: &QueryExecutionControls,
) -> Result<(), ParameterAdmissionError> {
    if controls.is_cancelled() {
        return Err(ParameterAdmissionError::Cancelled);
    }
    if controls.is_timed_out() {
        return Err(ParameterAdmissionError::Deadline);
    }
    Ok(())
}

pub(super) fn memory_error(error: CassieError) -> ParameterAdmissionError {
    // Only the existing query-memory reservation operations use this adapter.
    let message = match error {
        CassieError::ResourceLimit(message) => message,
        other => other.to_string(),
    };
    ParameterAdmissionError::Memory(message)
}

fn capacity_bytes<T>(capacity: usize) -> Result<usize, ParameterAdmissionError> {
    capacity.checked_mul(size_of::<T>()).ok_or_else(|| {
        ParameterAdmissionError::Memory("parameter metadata capacity overflow".to_owned())
    })
}

pub(super) struct ReservedVector<T> {
    pub(super) values: Vec<T>,
    pub(super) reservation: QueryMemoryReservation,
}

impl<T> ReservedVector<T> {
    pub(super) fn try_push(
        &mut self,
        value: T,
        controls: &QueryExecutionControls,
    ) -> Result<(), ParameterAdmissionError> {
        check_controls(controls)?;
        if self.values.len() == self.values.capacity() {
            let capacity = self.values.capacity();
            let growth = if capacity < 8 { 1 } else { capacity };
            let desired = capacity.checked_add(growth).ok_or_else(|| {
                ParameterAdmissionError::Memory("parameter fold capacity overflow".to_owned())
            })?;
            // A fresh buffer is admitted while the old backing is still live.
            let mut replacement = reserve_vector(controls, desired)?;
            check_controls(controls)?;
            replacement.values.append(&mut self.values);
            std::mem::swap(self, &mut replacement);
            // Old backing retires before its old reservation.
            drop(replacement);
        }
        self.values.push(value);
        Ok(())
    }
}

pub(super) fn reserve_vector<T>(
    controls: &QueryExecutionControls,
    capacity: usize,
) -> Result<ReservedVector<T>, ParameterAdmissionError> {
    check_controls(controls)?;
    let requested = capacity_bytes::<T>(capacity)?;
    let mut reservation = controls
        .reserve_query_memory(requested)
        .map_err(memory_error)?;
    check_controls(controls)?;
    let mut values = Vec::new();
    values.try_reserve_exact(capacity).map_err(|error| {
        ParameterAdmissionError::Memory(format!("unable to allocate parameter metadata: {error}"))
    })?;
    // Rust permits capacity surplus even for try_reserve_exact. Admit that
    // reported surplus before filling, or drop this empty allocation on error.
    let actual = capacity_bytes::<T>(values.capacity())?;
    if actual > requested {
        check_controls(controls)?;
        reservation
            .try_grow(actual - requested)
            .map_err(memory_error)?;
    }
    Ok(ReservedVector {
        values,
        reservation,
    })
}

pub(super) fn admit_invalid_boolean_literal(
    input: &str,
    controls: &QueryExecutionControls,
) -> Result<ParameterAdmissionError, ParameterAdmissionError> {
    check_controls(controls)?;
    let requested = input.len();
    let mut reservation = controls
        .reserve_query_memory(requested)
        .map_err(memory_error)?;
    check_controls(controls)?;
    let mut admitted_input = String::new();
    admitted_input
        .try_reserve_exact(requested)
        .map_err(|error| {
            ParameterAdmissionError::Memory(format!(
                "unable to allocate invalid Boolean input: {error}"
            ))
        })?;
    if admitted_input.capacity() > requested {
        check_controls(controls)?;
        reservation
            .try_grow(admitted_input.capacity() - requested)
            .map_err(memory_error)?;
    }
    check_controls(controls)?;
    admitted_input.push_str(input);
    Ok(ParameterAdmissionError::InvalidBooleanLiteral {
        input: admitted_input,
        _reservation: reservation,
    })
}

pub(super) fn admit_nonaggregate_modifier(
    function: &str,
    modifier: &'static str,
    controls: &QueryExecutionControls,
) -> Result<ParameterAdmissionError, ParameterAdmissionError> {
    check_controls(controls)?;
    let capacity = function
        .len()
        .checked_add(modifier.len())
        .and_then(|length| {
            length.checked_add(" specified, but  is not an aggregate function".len())
        })
        .ok_or_else(|| {
            ParameterAdmissionError::Memory("function diagnostic capacity overflow".into())
        })?;
    let mut reservation = controls
        .reserve_query_memory(capacity)
        .map_err(memory_error)?;
    check_controls(controls)?;
    let mut admitted = String::new();
    admitted.try_reserve_exact(capacity).map_err(|error| {
        ParameterAdmissionError::Memory(format!("unable to allocate function diagnostic: {error}"))
    })?;
    if admitted.capacity() > capacity {
        reservation
            .try_grow(admitted.capacity() - capacity)
            .map_err(memory_error)?;
    }
    check_controls(controls)?;
    admitted.push_str(modifier);
    admitted.push_str(" specified, but ");
    let start = admitted.len();
    admitted.push_str(function);
    admitted[start..].make_ascii_lowercase();
    admitted.push_str(" is not an aggregate function");
    Ok(ParameterAdmissionError::NonAggregateModifier {
        message: admitted,
        _reservation: reservation,
    })
}

struct WorkStack<'a, 'c> {
    frames: Vec<Frame<'a>>,
    reservation: QueryMemoryReservation,
    controls: &'c QueryExecutionControls,
}

impl<'a, 'c> WorkStack<'a, 'c> {
    fn new(controls: &'c QueryExecutionControls) -> Result<Self, ParameterAdmissionError> {
        check_controls(controls)?;
        Ok(Self {
            frames: Vec::new(),
            reservation: controls.reserve_query_memory(0).map_err(memory_error)?,
            controls,
        })
    }

    fn push(&mut self, frame: Frame<'a>) -> Result<(), ParameterAdmissionError> {
        check_controls(self.controls)?;
        if self.frames.len() == self.frames.capacity() {
            self.grow()?;
        }
        self.frames.push(frame);
        Ok(())
    }

    fn grow(&mut self) -> Result<(), ParameterAdmissionError> {
        check_controls(self.controls)?;
        let previous = self.reservation.bytes();
        let capacity = self.frames.capacity();
        let desired = capacity.checked_add(capacity.max(1)).ok_or_else(|| {
            ParameterAdmissionError::Memory("parameter walk capacity overflow".to_owned())
        })?;
        let requested = capacity_bytes::<Frame<'a>>(desired)?;
        self.reservation
            .try_grow(requested - previous)
            .map_err(memory_error)?;
        if let Err(error) = check_controls(self.controls) {
            self.reservation.shrink_to(previous);
            return Err(error);
        }
        if let Err(error) = self.frames.try_reserve_exact(desired - self.frames.len()) {
            self.reservation.shrink_to(previous);
            return Err(ParameterAdmissionError::Memory(format!(
                "unable to allocate parameter walk: {error}"
            )));
        }
        let actual = match capacity_bytes::<Frame<'a>>(self.frames.capacity()) {
            Ok(bytes) => bytes,
            Err(error) => {
                self.frames = Vec::new();
                self.reservation.shrink_to(0);
                return Err(error);
            }
        };
        if actual > requested {
            let admitted = check_controls(self.controls).and_then(|()| {
                self.reservation
                    .try_grow(actual - requested)
                    .map_err(memory_error)
            });
            if let Err(error) = admitted {
                // A capacity-reconciliation failure must not leave an enlarged
                // buffer alive with only the old requested charge.
                self.frames = Vec::new();
                self.reservation.shrink_to(0);
                return Err(error);
            }
        }
        Ok(())
    }
}

pub(super) fn checked_parameter_count(
    statement: &ParsedStatement,
    controls: &QueryExecutionControls,
) -> Result<usize, ParameterAdmissionError> {
    check_controls(controls)?;
    let mut stack = WorkStack::new(controls)?;
    stack.push(Frame::new(Node::Query(&statement.statement)))?;
    let mut count = 0;
    while let Some(frame) = stack.frames.last_mut() {
        // One borrowed child at a time: width never eagerly populates the stack.
        check_controls(controls)?;
        if !frame.entered {
            if let Node::Expr(ast::Expr::Param(index)) = frame.node {
                let extent = index
                    .checked_add(1)
                    .filter(|extent| *extent <= MAX_PARAMETER_COUNT)
                    .ok_or_else(|| {
                        ParameterAdmissionError::InvalidParam(format!(
                            "parameter index exceeds PostgreSQL limit {MAX_PARAMETER_COUNT}"
                        ))
                    })?;
                count = count.max(extent);
            }
            frame.entered = true;
        }
        if let Some(child) = frame.next_child() {
            stack.push(Frame::new(child))?;
        } else {
            stack.frames.pop();
        }
    }
    // Stack capacity remains charged through all pops until its owner drops.
    Ok(count)
}

#[derive(Clone, Copy)]
enum Node<'a> {
    Query(&'a QueryStatement),
    Select(&'a ast::SelectStatement),
    Source(&'a ast::QuerySource),
    Item(&'a ast::SelectItem),
    Cte(&'a ast::CteQuery),
    Expr(&'a ast::Expr),
    Function(&'a ast::FunctionCall),
    Expressions(&'a [ast::Expr]),
    Items(&'a [ast::SelectItem]),
    Orders(&'a [ast::OrderExpr]),
    Ctes(&'a [ast::CommonTableExpression]),
    Rows(&'a [Vec<ast::Expr>]),
    Assignments(&'a [(String, ast::Expr)]),
    Branches(&'a [(ast::Expr, ast::Expr)]),
    Pair(&'a ast::Expr, &'a ast::Expr),
    InsertSource(&'a ast::InsertSource),
    Conflict(&'a ast::InsertConflictAction),
}

struct Frame<'a> {
    node: Node<'a>,
    next: usize,
    entered: bool,
}

impl<'a> Frame<'a> {
    const fn new(node: Node<'a>) -> Self {
        Self {
            node,
            next: 0,
            entered: false,
        }
    }

    fn next_child(&mut self) -> Option<Node<'a>> {
        while self.next < self.node.children() {
            let index = self.next;
            self.next += 1;
            if let Some(child) = self.node.child(index) {
                return Some(child);
            }
        }
        None
    }
}

impl Node<'_> {
    fn children(self) -> usize {
        match self {
            Self::Query(query) => query_children(query),
            Self::Select(_) => 11,
            Self::Source(source) => match source {
                ast::QuerySource::Values { .. }
                | ast::QuerySource::Aliased { .. }
                | ast::QuerySource::TableFunction { .. }
                | ast::QuerySource::Subquery { .. } => 1,
                ast::QuerySource::Join { .. } => 3,
                ast::QuerySource::Collection(_)
                | ast::QuerySource::Cte(_)
                | ast::QuerySource::SingleRow => 0,
            },
            Self::Item(item) => match item {
                ast::SelectItem::Wildcard | ast::SelectItem::Column { .. } => 0,
                ast::SelectItem::Function { .. } | ast::SelectItem::Expr { .. } => 1,
                ast::SelectItem::WindowFunction { .. } => 3,
            },
            Self::Cte(cte) => match cte {
                ast::CteQuery::Simple(_) => 1,
                ast::CteQuery::Recursive { .. } => 2,
            },
            Self::Expr(expr) => expr_children(expr),
            Self::Function(_) => 3,
            Self::Expressions(values) => values.len(),
            Self::Items(values) => values.len(),
            Self::Orders(values) => values.len(),
            Self::Ctes(values) => values.len(),
            Self::Rows(values) => values.len(),
            Self::Assignments(values) => values.len(),
            Self::Branches(values) => values.len(),
            Self::Pair(_, _) => 2,
            Self::InsertSource(_) => 1,
            Self::Conflict(action) => match action {
                ast::InsertConflictAction::DoNothing => 0,
                ast::InsertConflictAction::DoUpdate { .. } => 2,
            },
        }
    }

    fn child(self, index: usize) -> Option<Self> {
        match self {
            Self::Query(query) => query_child(query, index),
            Self::Select(select) => select_child(select, index),
            Self::Source(source) => source_child(source, index),
            Self::Item(item) => item_child(item, index),
            Self::Cte(cte) => match cte {
                ast::CteQuery::Simple(statement) => Some(Self::Query(&statement.statement)),
                ast::CteQuery::Recursive {
                    base, recursive, ..
                } => {
                    let statement = if index == 0 { base } else { recursive };
                    Some(Self::Query(&statement.statement))
                }
            },
            Self::Expr(expr) => expr_child(expr, index),
            Self::Function(function) => match index {
                0 => Some(Self::Expressions(&function.args)),
                1 => function.filter.as_deref().map(Self::Expr),
                _ => Some(Self::Orders(&function.order_by)),
            },
            Self::Expressions(values) => values.get(index).map(Self::Expr),
            Self::Items(values) => values.get(index).map(Self::Item),
            Self::Orders(values) => values.get(index).map(|order| Self::Expr(&order.expr)),
            Self::Ctes(values) => values.get(index).map(|cte| Self::Cte(&cte.query)),
            Self::Rows(values) => values.get(index).map(|row| Self::Expressions(row)),
            Self::Assignments(values) => values.get(index).map(|(_, expr)| Self::Expr(expr)),
            Self::Branches(values) => values.get(index).map(|(when, then)| Self::Pair(when, then)),
            Self::Pair(first, second) => Some(Self::Expr(if index == 0 { first } else { second })),
            Self::InsertSource(source) => match source {
                ast::InsertSource::Values(rows) => Some(Self::Rows(rows)),
                ast::InsertSource::Select(select) => Some(Self::Select(select)),
            },
            Self::Conflict(action) => match action {
                ast::InsertConflictAction::DoNothing => None,
                ast::InsertConflictAction::DoUpdate {
                    assignments,
                    filter,
                } => match index {
                    0 => Some(Self::Assignments(assignments)),
                    _ => filter.as_ref().map(Self::Expr),
                },
            },
        }
    }
}

fn query_children(query: &QueryStatement) -> usize {
    match query {
        QueryStatement::Explain(_)
        | QueryStatement::Select(_)
        | QueryStatement::CallProcedure(_) => 1,
        QueryStatement::Insert(_) | QueryStatement::Update(_) => 3,
        QueryStatement::Delete(_) => 2,
        QueryStatement::Show(_)
        | QueryStatement::Set(_)
        | QueryStatement::Copy(_)
        | QueryStatement::Transaction(_)
        | QueryStatement::CreateTable(_)
        | QueryStatement::CreateGraph(_)
        | QueryStatement::DropTable(_)
        | QueryStatement::AlterTable(_)
        | QueryStatement::CreateSequence(_)
        | QueryStatement::DropSequence(_)
        | QueryStatement::CreateDatabase(_)
        | QueryStatement::DropDatabase(_)
        | QueryStatement::CreateSchema(_)
        | QueryStatement::CreateView(_)
        | QueryStatement::CreateRole(_)
        | QueryStatement::AlterRole(_)
        | QueryStatement::DropRole(_)
        | QueryStatement::GrantDatabaseConnect(_)
        | QueryStatement::RevokeDatabaseConnect(_)
        | QueryStatement::CreateIndex(_)
        | QueryStatement::DropIndex(_)
        | QueryStatement::CreateRollup(_)
        | QueryStatement::RefreshRollup(_)
        | QueryStatement::DropRollup(_)
        | QueryStatement::CreateMaterializedProjection(_)
        | QueryStatement::RefreshMaterializedProjection(_)
        | QueryStatement::DropMaterializedProjection(_)
        | QueryStatement::AlterMaterializedProjection(_)
        | QueryStatement::DropMaterializedProjectionVersion(_)
        | QueryStatement::VerifyProjection(_)
        | QueryStatement::DiffProjection(_)
        | QueryStatement::CompareProjection(_)
        | QueryStatement::PlanRepairProjection(_)
        | QueryStatement::RepairProjection(_)
        | QueryStatement::CreateRetentionPolicy(_)
        | QueryStatement::AlterRetentionPolicy(_)
        | QueryStatement::DropRetentionPolicy(_)
        | QueryStatement::EnforceRetentionPolicy(_)
        | QueryStatement::CreateFunction(_)
        | QueryStatement::DropFunction(_)
        | QueryStatement::CreateProcedure(_)
        | QueryStatement::DropProcedure(_)
        | QueryStatement::DropView(_)
        | QueryStatement::DropSchema(_)
        | QueryStatement::AlterSchema(_) => 0,
    }
}

fn query_child(query: &QueryStatement, index: usize) -> Option<Node<'_>> {
    match query {
        QueryStatement::Explain(explain) => Some(Node::Query(&explain.statement.statement)),
        QueryStatement::Select(select) => Some(Node::Select(select)),
        QueryStatement::CallProcedure(call) => Some(Node::Expressions(&call.args)),
        QueryStatement::Insert(insert) => match index {
            0 => Some(Node::InsertSource(&insert.source)),
            1 => insert
                .on_conflict
                .as_ref()
                .map(|clause| Node::Conflict(&clause.action)),
            _ => Some(Node::Items(&insert.returning)),
        },
        QueryStatement::Update(update) => match index {
            0 => Some(Node::Assignments(&update.assignments)),
            1 => update.filter.as_ref().map(Node::Expr),
            _ => Some(Node::Items(&update.returning)),
        },
        QueryStatement::Delete(delete) => match index {
            0 => delete.filter.as_ref().map(Node::Expr),
            _ => Some(Node::Items(&delete.returning)),
        },
        // Preserve current count.rs ignored-DDL semantics exhaustively.
        QueryStatement::Show(_)
        | QueryStatement::Set(_)
        | QueryStatement::Copy(_)
        | QueryStatement::Transaction(_)
        | QueryStatement::CreateTable(_)
        | QueryStatement::CreateGraph(_)
        | QueryStatement::DropTable(_)
        | QueryStatement::AlterTable(_)
        | QueryStatement::CreateSequence(_)
        | QueryStatement::DropSequence(_)
        | QueryStatement::CreateDatabase(_)
        | QueryStatement::DropDatabase(_)
        | QueryStatement::CreateSchema(_)
        | QueryStatement::CreateView(_)
        | QueryStatement::CreateRole(_)
        | QueryStatement::AlterRole(_)
        | QueryStatement::DropRole(_)
        | QueryStatement::GrantDatabaseConnect(_)
        | QueryStatement::RevokeDatabaseConnect(_)
        | QueryStatement::CreateIndex(_)
        | QueryStatement::DropIndex(_)
        | QueryStatement::CreateRollup(_)
        | QueryStatement::RefreshRollup(_)
        | QueryStatement::DropRollup(_)
        | QueryStatement::CreateMaterializedProjection(_)
        | QueryStatement::RefreshMaterializedProjection(_)
        | QueryStatement::DropMaterializedProjection(_)
        | QueryStatement::AlterMaterializedProjection(_)
        | QueryStatement::DropMaterializedProjectionVersion(_)
        | QueryStatement::VerifyProjection(_)
        | QueryStatement::DiffProjection(_)
        | QueryStatement::CompareProjection(_)
        | QueryStatement::PlanRepairProjection(_)
        | QueryStatement::RepairProjection(_)
        | QueryStatement::CreateRetentionPolicy(_)
        | QueryStatement::AlterRetentionPolicy(_)
        | QueryStatement::DropRetentionPolicy(_)
        | QueryStatement::EnforceRetentionPolicy(_)
        | QueryStatement::CreateFunction(_)
        | QueryStatement::DropFunction(_)
        | QueryStatement::CreateProcedure(_)
        | QueryStatement::DropProcedure(_)
        | QueryStatement::DropView(_)
        | QueryStatement::DropSchema(_)
        | QueryStatement::AlterSchema(_) => None,
    }
}

fn select_child(select: &ast::SelectStatement, index: usize) -> Option<Node<'_>> {
    match index {
        0 => Some(Node::Source(&select.source)),
        1 => Some(Node::Ctes(&select.ctes)),
        2 => Some(Node::Items(&select.projection)),
        3 => select.filter.as_ref().map(Node::Expr),
        4 => Some(Node::Expressions(&select.distinct_on)),
        5 => Some(Node::Expressions(&select.group_by)),
        6 => select.having.as_ref().map(Node::Expr),
        7 => Some(Node::Orders(&select.order)),
        8 => select.limit.as_ref().map(Node::Expr),
        9 => select.offset.as_ref().map(Node::Expr),
        _ => select.set.as_ref().map(|set| Node::Select(&set.right)),
    }
}

fn source_child(source: &ast::QuerySource, index: usize) -> Option<Node<'_>> {
    match source {
        ast::QuerySource::Values { rows } => Some(Node::Rows(rows)),

        ast::QuerySource::Aliased { source, .. } => Some(Node::Source(source)),
        ast::QuerySource::TableFunction { function, .. } => Some(Node::Function(function)),
        ast::QuerySource::Subquery { select, .. } => Some(Node::Select(select)),
        ast::QuerySource::Join {
            left, right, on, ..
        } => Some(match index {
            0 => Node::Source(left),
            1 => Node::Source(right),
            _ => Node::Expr(on),
        }),
        ast::QuerySource::Collection(_)
        | ast::QuerySource::Cte(_)
        | ast::QuerySource::SingleRow => None,
    }
}

fn item_child(item: &ast::SelectItem, index: usize) -> Option<Node<'_>> {
    match item {
        ast::SelectItem::Wildcard | ast::SelectItem::Column { .. } => None,
        ast::SelectItem::Function { function, .. } => Some(Node::Function(function)),
        ast::SelectItem::Expr { expr, .. } => Some(Node::Expr(expr)),
        ast::SelectItem::WindowFunction { function, .. } => Some(match index {
            0 => Node::Expressions(&function.args),
            1 => Node::Expressions(&function.partition_by),
            _ => Node::Orders(&function.order_by),
        }),
    }
}

fn expr_children(expr: &ast::Expr) -> usize {
    match expr {
        ast::Expr::Column(_)
        | ast::Expr::Param(_)
        | ast::Expr::StringLiteral(_)
        | ast::Expr::NumberLiteral(_)
        | ast::Expr::IntegerLiteral(_)
        | ast::Expr::NumericLiteral(_)
        | ast::Expr::BoolLiteral(_)
        | ast::Expr::Null => 0,
        ast::Expr::Case { .. } | ast::Expr::Between { .. } => 3,
        ast::Expr::RowCompare(comparison) => comparison.left.len() + comparison.right.len(),
        ast::Expr::Binary { .. } | ast::Expr::InList { .. } => 2,
        ast::Expr::IsNull { .. }
        | ast::Expr::Not { .. }
        | ast::Expr::UnaryNumeric { .. }
        | ast::Expr::IntegerNegate { .. }
        | ast::Expr::Cast { .. }
        | ast::Expr::Exists(_)
        | ast::Expr::Function(_) => 1,
    }
}

fn expr_child(expr: &ast::Expr, index: usize) -> Option<Node<'_>> {
    match expr {
        ast::Expr::Case {
            operand,
            branches,
            else_expr,
        } => match index {
            0 => operand.as_deref().map(Node::Expr),
            1 => Some(Node::Branches(branches)),
            _ => else_expr.as_deref().map(Node::Expr),
        },
        ast::Expr::RowCompare(comparison) => comparison
            .left
            .get(index)
            .or_else(|| {
                index
                    .checked_sub(comparison.left.len())
                    .and_then(|index| comparison.right.get(index))
            })
            .map(Node::Expr),
        ast::Expr::Binary { left, right, .. } => {
            Some(Node::Expr(if index == 0 { left } else { right }))
        }
        ast::Expr::IsNull { expr, .. }
        | ast::Expr::Not { expr }
        | ast::Expr::Cast { expr, .. }
        | ast::Expr::UnaryNumeric { expr, .. }
        | ast::Expr::IntegerNegate { expr, .. } => Some(Node::Expr(expr)),
        ast::Expr::InList { expr, values, .. } => Some(if index == 0 {
            Node::Expr(expr)
        } else {
            Node::Expressions(values)
        }),
        ast::Expr::Between {
            expr, low, high, ..
        } => Some(Node::Expr(match index {
            0 => expr,
            1 => low,
            _ => high,
        })),
        ast::Expr::Exists(statement) => Some(Node::Query(&statement.statement)),
        ast::Expr::Function(function) => Some(Node::Function(function)),
        ast::Expr::Column(_)
        | ast::Expr::Param(_)
        | ast::Expr::StringLiteral(_)
        | ast::Expr::NumberLiteral(_)
        | ast::Expr::IntegerLiteral(_)
        | ast::Expr::NumericLiteral(_)
        | ast::Expr::BoolLiteral(_)
        | ast::Expr::Null => None,
    }
}

#[cfg(test)]
#[path = "admission_tests.rs"]
mod tests;
