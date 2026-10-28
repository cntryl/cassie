//! Borrowed child scheduling; Boolean arguments are coerced before their sibling.

use super::{
    BooleanConstruct, Expected, Fold, Node, ParameterAdmissionError, UNKNOWN_PARAMETER_TYPE_OID,
};
use crate::sql::{
    ast, binder,
    functions::{self, WindowArgumentType},
    QueryStatement,
};

pub(super) struct Child<'a> {
    pub(super) node: Node<'a>,
    pub(super) context: usize,
    pub(super) expected: Expected,
    pub(super) target: bool,
    pub(super) qualified: bool,
}

impl<'a> Child<'a> {
    fn node(node: Node<'a>, context: usize, qualified: bool) -> Self {
        Self {
            node,
            context,
            expected: Expected::None,
            target: false,
            qualified,
        }
    }
}

impl<'a> Fold<'a, '_> {
    pub(super) fn child(
        &mut self,
        frame_index: usize,
    ) -> Result<Option<Child<'a>>, ParameterAdmissionError> {
        loop {
            let (parent_node, index, context, qualified, start) = {
                let frame = &mut self.frames.values[frame_index];
                if frame.next >= children(frame.node)? {
                    return Ok(None);
                }
                let index = frame.next;
                frame.next += 1;
                (
                    frame.node,
                    index,
                    frame.context,
                    frame.qualified,
                    frame.start,
                )
            };
            let window_value_oid = self.window_value_oid(parent_node, index, context);
            let window_default_oid = self.window_default_oid(parent_node, index, start);
            let child = match parent_node {
                Node::Query(query) => query_child(query),
                Node::Select(select) => select_child(select, index),
                Node::Source(source) => source_child(source, index),
                Node::Item(item) => item_child(item),
                Node::Expr(expr) => expression_child(expr, index),
                Node::Function(function) => {
                    function.args.get(index).map(Node::Expr).or_else(|| {
                        (index == function.args.len()).then_some(Node::FunctionModifiers(function))
                    })
                }
                Node::FunctionModifiers(function) => match index {
                    0 => function.filter.as_deref().map(Node::Expr),
                    _ => Some(Node::Orders(&function.order_by)),
                },
                Node::Expressions(expressions) => expressions.get(index).map(Node::Expr),
                Node::Items(items) => items.get(index).map(Node::Item),
                Node::Orders(orders) => orders.get(index).map(|order| Node::Expr(&order.expr)),
                Node::Ctes(ctes) => ctes.get(index).map(|cte| Node::Cte(&cte.query)),
                Node::Cte(cte) => Some(cte_child(cte, index)),
                Node::Window(window) => match index {
                    0 => Some(Node::WindowArguments(window)),
                    1 => Some(Node::Expressions(&window.partition_by)),
                    _ => Some(Node::Orders(&window.order_by)),
                },
                Node::WindowArguments(window) => window.args.get(index).map(Node::Expr),
            };
            let Some(node) = child else {
                continue;
            };
            let mut child = Child::node(node, context, qualified);
            qualify_child(parent_node, index, &mut child);
            if let Some(oid) =
                window_default_oid.filter(|oid| !matches!(*oid, 0 | UNKNOWN_PARAMETER_TYPE_OID))
            {
                child.expected = Expected::Type(oid, true);
                child.qualified = false;
            }
            if let Some(oid) = window_value_oid.filter(|oid| {
                matches!(child.node, Node::Expr(ast::Expr::Param(_)))
                    && !matches!(*oid, 0 | UNKNOWN_PARAMETER_TYPE_OID)
            }) {
                child.expected = Expected::Type(oid, true);
                child.qualified = false;
            }
            return Ok(Some(child));
        }
    }

    fn window_value_oid(&self, node: Node<'_>, index: usize, context: usize) -> Option<i32> {
        let Node::WindowArguments(window) = node else {
            return None;
        };
        let metadata = functions::window_function(&window.name)?;
        if index != 0
            || !matches!(
                metadata.id,
                functions::FunctionId::Lag | functions::FunctionId::Lead
            )
            || !matches!(
                window_argument_type(window, 0),
                Some(WindowArgumentType::AnyElement | WindowArgumentType::AnyCompatible)
            )
        {
            return None;
        }
        let parameters = &self.occurrences.slots.values;
        let default_oid = (window_argument_type(window, 2)
            == Some(WindowArgumentType::AnyCompatible))
        .then(|| window.args.get(2))
        .flatten()
        .and_then(|default| {
            known_window_argument_oid(default, &self.contexts.values[context].fields, parameters)
        });
        default_oid
            .filter(|oid| !matches!(*oid, 0 | UNKNOWN_PARAMETER_TYPE_OID))
            .or_else(|| {
                window
                    .args
                    .first()
                    .zip(window.args.get(2))
                    .is_some_and(|(value, default)| {
                        unknown_window_argument(value, parameters)
                            && unknown_window_argument(default, parameters)
                    })
                    .then_some(25)
            })
    }

    fn window_default_oid(&self, node: Node<'_>, index: usize, start: usize) -> Option<i32> {
        let Node::WindowArguments(window) = node else {
            return None;
        };
        let metadata = functions::window_function(&window.name)?;
        if index != 2
            || !matches!(
                metadata.id,
                functions::FunctionId::Lag | functions::FunctionId::Lead
            )
            || window_argument_type(window, 2) != Some(WindowArgumentType::AnyCompatible)
        {
            return None;
        }
        let value_oid = self
            .summaries
            .values
            .get(start)
            .map(|summary| summary.oid)
            .filter(|oid| !matches!(*oid, 0 | UNKNOWN_PARAMETER_TYPE_OID));
        value_oid.or_else(|| {
            let parameters = &self.occurrences.slots.values;
            window
                .args
                .first()
                .zip(window.args.get(2))
                .is_some_and(|(value, default)| {
                    unknown_window_argument(value, parameters)
                        && unknown_window_argument(default, parameters)
                })
                .then_some(25)
        })
    }
}

fn known_window_argument_oid(
    expr: &ast::Expr,
    fields: &crate::sql::FieldTypeMap,
    parameter_types: &[i32],
) -> Option<i32> {
    match expr {
        ast::Expr::Param(index) => parameter_types.get(*index).copied(),
        ast::Expr::StringLiteral(_) | ast::Expr::Null => None,
        _ => binder::known_expr_type(expr, fields)
            .and_then(|data_type| i32::try_from(data_type.type_oid()).ok()),
    }
}

fn unknown_window_argument(expr: &ast::Expr, parameter_types: &[i32]) -> bool {
    match expr {
        ast::Expr::Param(index) => parameter_types
            .get(*index)
            .is_none_or(|oid| matches!(*oid, 0 | UNKNOWN_PARAMETER_TYPE_OID)),
        ast::Expr::StringLiteral(_) | ast::Expr::Null => true,
        _ => false,
    }
}

fn window_argument_type(
    window: &ast::WindowFunctionCall,
    index: usize,
) -> Option<WindowArgumentType> {
    functions::window_function(&window.name)?
        .signature(window.args.len())?
        .get(index)
        .copied()
}

fn children(node: Node<'_>) -> Result<usize, ParameterAdmissionError> {
    Ok(match node {
        Node::Query(_) | Node::Item(_) => 1,
        Node::Select(_) => 11,
        Node::Source(source) => match source {
            ast::QuerySource::Values { rows } => rows
                .len()
                .checked_mul(rows.first().map_or(0, Vec::len))
                .ok_or_else(|| {
                    ParameterAdmissionError::Memory("VALUES expression count overflow".into())
                })?,
            ast::QuerySource::Join { .. } => 3,
            ast::QuerySource::TableSample { repeatable, .. } => {
                2 + usize::from(repeatable.is_some())
            }
            ast::QuerySource::Aliased { .. } | ast::QuerySource::Subquery { .. } => 1,
            ast::QuerySource::Collection(_)
            | ast::QuerySource::Cte(_)
            | ast::QuerySource::SingleRow
            | ast::QuerySource::TableFunction { .. } => 0,
        },
        Node::Expr(expr) => expression_children(expr),
        Node::Function(function) => function.args.len() + 1,
        Node::FunctionModifiers(_) => 2,
        Node::Expressions(expressions) => expressions.len(),
        Node::Items(items) => items.len(),
        Node::Orders(orders) => orders.len(),
        Node::Ctes(ctes) => ctes.len(),
        Node::Cte(cte) => {
            if matches!(cte, ast::CteQuery::Simple(_)) {
                1
            } else {
                2
            }
        }
        Node::Window(_) => 3,
        Node::WindowArguments(window) => window.args.len(),
    })
}

fn query_child(query: &QueryStatement) -> Option<Node<'_>> {
    match query {
        QueryStatement::Select(select) => Some(Node::Select(select)),
        QueryStatement::Explain(explain) => Some(Node::Query(&explain.statement.statement)),
        // Non-SELECT statement transformation retains its explicit legacy route.
        QueryStatement::Insert(_)
        | QueryStatement::Update(_)
        | QueryStatement::Delete(_)
        | QueryStatement::Truncate(_)
        | QueryStatement::Merge(_)
        | QueryStatement::CallProcedure(_)
        | QueryStatement::Show(_)
        | QueryStatement::Set(_)
        | QueryStatement::Copy(_)
        | QueryStatement::Transaction(_)
        | QueryStatement::SessionObject(_)
        | QueryStatement::CreateTable(_)
        | QueryStatement::CreateGraph(_)
        | QueryStatement::DropTable(_)
        | QueryStatement::AlterTable(_)
        | QueryStatement::CreateSequence(_)
        | QueryStatement::AlterSequence(_)
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
        | QueryStatement::AlterIndex(_)
        | QueryStatement::Reindex(_)
        | QueryStatement::CreateRollup(_)
        | QueryStatement::RefreshRollup(_)
        | QueryStatement::DropRollup(_)
        | QueryStatement::CreateMaterializedProjection(_)
        | QueryStatement::RefreshMaterializedProjection(_)
        | QueryStatement::DropMaterializedProjection(_)
        | QueryStatement::CreateMaterializedView(_)
        | QueryStatement::RefreshMaterializedView(_)
        | QueryStatement::DropMaterializedView(_)
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
        0 => Some(Node::Ctes(&select.ctes)),
        1 => Some(Node::Source(&select.source)),
        2 => Some(Node::Items(&select.projection)),
        3 => select.filter.as_ref().map(Node::Expr),
        4 => select.having.as_ref().map(Node::Expr),
        5 => Some(Node::Orders(&select.order)),
        6 => Some(Node::Expressions(&select.group_by)),
        7 => Some(Node::Expressions(&select.distinct_on)),
        8 => select.offset.as_ref().map(Node::Expr),
        9 => select.limit.as_ref().map(Node::Expr),
        _ => select.set.as_ref().map(|set| Node::Select(&set.right)),
    }
}

fn source_child(source: &ast::QuerySource, index: usize) -> Option<Node<'_>> {
    match source {
        ast::QuerySource::Values { rows } => {
            let width = rows.first()?.len();
            rows.get(index / width)?.get(index % width).map(Node::Expr)
        }

        ast::QuerySource::Aliased { source, .. } => Some(Node::Source(source)),
        ast::QuerySource::TableSample {
            source,
            rate,
            repeatable,
            ..
        } => Some(match index {
            0 => Node::Source(source),
            1 => Node::Expr(rate),
            _ => Node::Expr(repeatable.as_ref()?),
        }),
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
        | ast::QuerySource::SingleRow
        | ast::QuerySource::TableFunction { .. } => None,
    }
}

fn item_child(item: &ast::SelectItem) -> Option<Node<'_>> {
    match item {
        ast::SelectItem::Expr { expr, .. } => Some(Node::Expr(expr)),
        ast::SelectItem::Function { function, .. } => Some(Node::Function(function)),
        ast::SelectItem::WindowFunction { function, .. } => Some(Node::Window(function)),
        ast::SelectItem::Wildcard | ast::SelectItem::Column { .. } => None,
    }
}

fn cte_child(cte: &ast::CteQuery, index: usize) -> Node<'_> {
    match cte {
        ast::CteQuery::Simple(statement) => Node::Query(&statement.statement),
        ast::CteQuery::Recursive {
            base, recursive, ..
        } => Node::Query(
            &if index == 0 {
                base.as_ref()
            } else {
                recursive.as_ref()
            }
            .statement,
        ),
    }
}

fn expression_children(expr: &ast::Expr) -> usize {
    match expr {
        ast::Expr::Column(_)
        | ast::Expr::Param(_)
        | ast::Expr::StringLiteral(_)
        | ast::Expr::NumberLiteral(_)
        | ast::Expr::IntegerLiteral(_)
        | ast::Expr::NumericLiteral(_)
        | ast::Expr::BoolLiteral(_)
        | ast::Expr::Null => 0,
        ast::Expr::RowCompare(comparison) => comparison.left.len() + comparison.right.len(),
        ast::Expr::ArraySubscript { subscripts, .. } => {
            1 + subscripts
                .iter()
                .map(|subscript| match subscript {
                    ast::ArraySubscript::Index(_) => 1,
                    ast::ArraySubscript::Slice { lower, upper } => {
                        usize::from(lower.is_some()) + usize::from(upper.is_some())
                    }
                })
                .sum::<usize>()
        }
        ast::Expr::Binary { .. } => 2,
        ast::Expr::Between { .. } | ast::Expr::Like { .. } => 3,
        ast::Expr::Case { branches, .. } => 2 + branches.len() * 2,
        ast::Expr::InList { values, .. } => 1 + values.len(),
        ast::Expr::IsNull { .. }
        | ast::Expr::TruthTest { .. }
        | ast::Expr::Not { .. }
        | ast::Expr::UnaryNumeric { .. }
        | ast::Expr::IntegerNegate { .. }
        | ast::Expr::Cast { .. }
        | ast::Expr::Collate { .. }
        | ast::Expr::Exists(_)
        | ast::Expr::Function(_) => 1,
        ast::Expr::Subquery(subquery) => subquery.operands().len() + 1,
    }
}

fn expression_child(expr: &ast::Expr, index: usize) -> Option<Node<'_>> {
    match expr {
        ast::Expr::ArraySubscript { array, subscripts } => {
            if index == 0 {
                Some(Node::Expr(array))
            } else {
                subscript_operand(subscripts, index - 1).map(Node::Expr)
            }
        }
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
        | ast::Expr::TruthTest { expr, .. }
        | ast::Expr::Not { expr }
        | ast::Expr::Cast { expr, .. }
        | ast::Expr::Collate { expr, .. }
        | ast::Expr::UnaryNumeric { expr, .. }
        | ast::Expr::IntegerNegate { expr, .. } => Some(Node::Expr(expr)),
        ast::Expr::Exists(statement) => Some(Node::Query(&statement.statement)),
        ast::Expr::Subquery(subquery) => Some(
            subquery
                .operands()
                .get(index)
                .map_or(Node::Query(&subquery.query.statement), Node::Expr),
        ),
        ast::Expr::Function(function) => Some(Node::Function(function)),
        ast::Expr::Between {
            expr, low, high, ..
        }
        | ast::Expr::Like {
            expr,
            pattern: low,
            escape: high,
            ..
        } => Some(Node::Expr(match index {
            0 => expr,
            1 => low,
            _ => high,
        })),
        ast::Expr::InList { expr, values, .. } => {
            if index == 0 {
                Some(Node::Expr(expr))
            } else {
                values.get(index - 1).map(Node::Expr)
            }
        }
        ast::Expr::Case {
            operand,
            branches,
            else_expr,
        } => {
            if index == 0 {
                operand.as_deref().map(Node::Expr)
            } else if let Some((when, then)) = branches.get((index - 1) / 2) {
                Some(Node::Expr(if index % 2 == 1 { when } else { then }))
            } else {
                else_expr.as_deref().map(Node::Expr)
            }
        }
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

fn subscript_operand(subscripts: &[ast::ArraySubscript], mut index: usize) -> Option<&ast::Expr> {
    for subscript in subscripts {
        match subscript {
            ast::ArraySubscript::Index(expression) => {
                if index == 0 {
                    return Some(expression);
                }
                index -= 1;
            }
            ast::ArraySubscript::Slice { lower, upper } => {
                for bound in lower.iter().chain(upper) {
                    if index == 0 {
                        return Some(bound);
                    }
                    index -= 1;
                }
            }
        }
    }
    None
}

fn qualify_child(parent: Node<'_>, index: usize, child: &mut Child<'_>) {
    match parent {
        Node::Items(_) => child.target = true,
        Node::FunctionModifiers(_) if index == 0 => {
            child.expected = Expected::Boolean(BooleanConstruct::Filter);
        }
        Node::Select(_) => {
            child.expected = match index {
                3 => Expected::Boolean(BooleanConstruct::Where),
                4 => Expected::Boolean(BooleanConstruct::Having),
                8 | 9 => Expected::Type(20, true),
                _ => Expected::None,
            }
        }
        Node::Source(ast::QuerySource::Join { .. }) if index == 2 => {
            child.expected = Expected::Boolean(BooleanConstruct::Join);
        }
        Node::Source(ast::QuerySource::TableSample { repeatable, .. }) => {
            child.expected = match index {
                1 => Expected::Type(700, true),
                2 if repeatable.is_some() => Expected::Type(701, true),
                _ => Expected::None,
            };
        }
        Node::Expr(ast::Expr::Binary {
            op: ast::BinaryOp::Integer { operation, .. },
            ..
        }) if operation.is_shift() && index == 1 => {
            child.expected = Expected::Type(23, true);
        }
        Node::Expr(ast::Expr::ArraySubscript { .. }) if index > 0 => {
            child.expected = Expected::Type(23, true);
        }
        Node::Expr(
            ast::Expr::Binary {
                op: ast::BinaryOp::Integer { width, .. },
                ..
            }
            | ast::Expr::IntegerNegate { width, .. },
        ) => {
            child.expected = Expected::Type(super::expressions::oid(&width.data_type()), true);
        }
        Node::Expr(ast::Expr::UnaryNumeric {
            op: ast::UnaryNumericOp::SquareRoot | ast::UnaryNumericOp::CubeRoot,
            ..
        }) => {
            child.expected = Expected::Type(701, true);
        }
        Node::Expr(ast::Expr::TruthTest { truth, negated, .. }) => {
            child.expected = Expected::Boolean(BooleanConstruct::Truth(*truth, *negated));
        }
        Node::Expr(ast::Expr::Not { .. }) => {
            child.expected = Expected::Boolean(BooleanConstruct::Not);
        }
        Node::Expr(ast::Expr::Binary {
            op: ast::BinaryOp::And,
            ..
        }) => child.expected = Expected::Boolean(BooleanConstruct::And),
        Node::Expr(ast::Expr::Binary {
            op: ast::BinaryOp::Or,
            ..
        }) => child.expected = Expected::Boolean(BooleanConstruct::Or),
        Node::Expr(ast::Expr::Cast { data_type, .. }) => {
            let oid = i32::try_from(data_type.type_oid()).unwrap_or(i32::MAX);
            // Explicit constants are converted by the ordered binder. Parameter
            // occurrences still receive the cast's concrete input identity here.
            child.expected = if matches!(child.node, Node::Expr(ast::Expr::StringLiteral(_))) {
                Expected::None
            } else {
                Expected::Type(oid, false)
            };
            child.qualified &= super::super::occurrences::primitive_oid(oid);
        }
        Node::Expr(ast::Expr::Case {
            operand, branches, ..
        }) => {
            if operand.is_none() && index % 2 == 1 && index <= branches.len() * 2 {
                child.expected = Expected::Boolean(BooleanConstruct::CaseWhen);
            } else {
                child.qualified = false;
            }
        }
        Node::WindowArguments(window) => {
            let int4_argument = index == 0 && window.name.eq_ignore_ascii_case("ntile")
                || window_argument_type(window, index) == Some(WindowArgumentType::Int4);
            if int4_argument {
                child.expected = Expected::Type(23, true);
            }
            child.qualified = false;
        }
        Node::Expr(
            ast::Expr::Binary { .. }
            | ast::Expr::Between { .. }
            | ast::Expr::InList { .. }
            | ast::Expr::Like { .. },
        )
        | Node::Window(_)
        | Node::Cte(ast::CteQuery::Recursive { .. }) => child.qualified = false,
        _ => {}
    }
}
