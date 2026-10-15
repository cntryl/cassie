//! One-child-at-a-time transformation in SELECT analysis order.

use crate::catalog::{Catalog, FunctionInputScope};
use crate::runtime::QueryExecutionControls;
use crate::sql::{ast, FieldTypeMap, QueryStatement};

use super::admission::{check_controls, reserve_vector, ReservedVector};
use super::diagnostics::BooleanConstruct;
use super::occurrences::{Occurrences, PrivateParameterTypeInput, Summary};
use super::ParameterAdmissionError;

#[path = "fold_columns.rs"]
mod columns;
#[path = "fold_expressions.rs"]
mod expressions;
#[path = "fold_functions.rs"]
mod functions;
#[path = "fold_nodes.rs"]
mod nodes;

#[derive(Clone, Copy)]
enum Expected {
    None,
    Boolean(BooleanConstruct),
    Type(i32, bool),
}

#[derive(Clone, Copy)]
enum Node<'a> {
    Query(&'a QueryStatement),
    Select(&'a ast::SelectStatement),
    Source(&'a ast::QuerySource),
    Item(&'a ast::SelectItem),
    Expr(&'a ast::Expr),
    Function(&'a ast::FunctionCall),
    FunctionModifiers(&'a ast::FunctionCall),
    Expressions(&'a [ast::Expr]),
    Items(&'a [ast::SelectItem]),
    Orders(&'a [ast::OrderExpr]),
    Ctes(&'a [ast::CommonTableExpression]),
    Cte(&'a ast::CteQuery),
    Window(&'a ast::WindowFunctionCall),
}

struct Frame<'a> {
    node: Node<'a>,
    next: usize,
    start: usize,
    context: usize,
    expected: Expected,
    target: bool,
    qualified: bool,
    occurrence_start: usize,
}

struct Context<'a> {
    fields: FieldTypeMap,
    source: &'a ast::QuerySource,
    target_start: usize,
}

struct Target {
    summary: Summary,
    context: usize,
}

pub(super) struct Fold<'a, 'c> {
    occurrences: Occurrences<'a>,
    frames: ReservedVector<Frame<'a>>,
    summaries: ReservedVector<Summary>,
    targets: ReservedVector<Target>,
    contexts: ReservedVector<Context<'a>>,
    catalog: &'c Catalog,
    controls: &'c QueryExecutionControls,
    scope: Option<&'c FunctionInputScope<'c>>,
}

impl<'a, 'c> Fold<'a, 'c> {
    pub(super) fn new(
        count: usize,
        input: PrivateParameterTypeInput<'a>,
        catalog: &'c Catalog,
        controls: &'c QueryExecutionControls,
        scope: Option<&'c FunctionInputScope<'c>>,
        stop_on_error: bool,
    ) -> Result<Self, ParameterAdmissionError> {
        Ok(Self {
            occurrences: Occurrences::new(count, input, controls, stop_on_error)?,
            frames: reserve_vector(controls, 0)?,
            summaries: reserve_vector(controls, 0)?,
            targets: reserve_vector(controls, 0)?,
            contexts: reserve_vector(controls, 0)?,
            catalog,
            controls,
            scope,
        })
    }

    pub(super) fn run(
        mut self,
        query: &'a QueryStatement,
    ) -> Result<Occurrences<'a>, ParameterAdmissionError> {
        self.push(Node::Query(query), 0, Expected::None, false, true)?;
        while !self.frames.values.is_empty() {
            check_controls(self.controls)?;
            let index = self.frames.values.len() - 1;
            if let Some(child) = self.child(index)? {
                self.push(
                    child.node,
                    child.context,
                    child.expected,
                    child.target,
                    child.qualified,
                )?;
            } else {
                let frame = self.frames.values.pop().expect("nonempty fold stack");
                self.finish(&frame)?;
            }
        }
        self.occurrences.finish(self.controls)?;
        Ok(self.occurrences)
    }

    fn push(
        &mut self,
        node: Node<'a>,
        mut context: usize,
        expected: Expected,
        target: bool,
        qualified: bool,
    ) -> Result<(), ParameterAdmissionError> {
        if let Node::Select(select) = node {
            context = self.contexts.values.len();
            // Source metadata discovery is the existing catalog-input boundary.
            // New fold directories and retained summaries are admitted separately.
            let fields = crate::sql::source_field_type_map_with_ctes(
                &select.source,
                &select.ctes,
                self.catalog,
            );
            self.contexts.try_push(
                Context {
                    fields,
                    source: &select.source,
                    target_start: self.targets.values.len(),
                },
                self.controls,
            )?;
        }
        self.frames.try_push(
            Frame {
                node,
                next: 0,
                start: self.summaries.values.len(),
                context,
                expected,
                target,
                qualified,
                occurrence_start: self.occurrences.len(),
            },
            self.controls,
        )
    }

    fn finish(&mut self, frame: &Frame<'a>) -> Result<(), ParameterAdmissionError> {
        if let Node::Expr(ast::Expr::StringLiteral(input)) = frame.node {
            if matches!(frame.expected, Expected::Boolean(_) | Expected::Type(16, _)) {
                self.occurrences
                    .invalid_boolean_literal(input, self.controls)?;
            }
        }
        let mut summary = match frame.node {
            Node::Expr(expr) => Some(self.expression(expr, frame)?),
            Node::Function(function) => {
                let summary = self.function(function, frame)?;
                self.invalid_boolean_function_literals(function, summary, frame)?;
                Some(summary)
            }
            Node::Item(item) => match item {
                ast::SelectItem::Expr { .. } | ast::SelectItem::Function { .. } => {
                    self.summaries.values.get(frame.start).copied()
                }
                ast::SelectItem::Wildcard
                | ast::SelectItem::Column { .. }
                | ast::SelectItem::WindowFunction { .. } => None,
            },
            Node::Select(_) => {
                let start = self.contexts.values[frame.context].target_start;
                for index in start..self.targets.values.len() {
                    check_controls(self.controls)?;
                    let target = &mut self.targets.values[index];
                    debug_assert_eq!(target.context, frame.context);
                    self.occurrences.finalize_target(&mut target.summary)?;
                }
                self.targets.values.truncate(start);
                self.contexts.values.pop();
                None
            }
            Node::Query(_)
            | Node::Source(_)
            | Node::Expressions(_)
            | Node::Items(_)
            | Node::Orders(_)
            | Node::Ctes(_)
            | Node::Cte(_)
            | Node::FunctionModifiers(_)
            | Node::Window(_) => None,
        };
        self.summaries.values.truncate(frame.start);
        if let Some(summary) = &mut summary {
            match frame.expected {
                Expected::None => {}
                Expected::Boolean(construct) => {
                    self.occurrences
                        .boolean(summary, construct, frame.qualified)?;
                }
                Expected::Type(oid, demand) => self.occurrences.coerce(summary, oid, demand)?,
            }
            if frame.target {
                self.targets.try_push(
                    Target {
                        summary: *summary,
                        context: frame.context,
                    },
                    self.controls,
                )?;
            } else {
                self.summaries.try_push(*summary, self.controls)?;
            }
        }
        Ok(())
    }

    fn fields(&self, context: usize) -> &FieldTypeMap {
        &self.contexts.values[context].fields
    }
}
