//! Per-field analyzer, vocabulary and scoring requirements for this query level.
use super::{Expr, FunctionCall, HashSet, LogicalPlan, SelectItem};
use crate::types::Value;

#[derive(Default)]
pub(in crate::executor::execution) struct SearchFields {
    pub(in crate::executor::execution) scored: HashSet<String>,
    pub(in crate::executor::execution) snippets: HashSet<String>,
    pub(in crate::executor::execution) expanded: HashSet<String>,
}

impl SearchFields {
    pub(in crate::executor::execution) fn for_plan(plan: &LogicalPlan, params: &[Value]) -> Self {
        let mut fields = Self::default();
        for expr in plan
            .filter
            .iter()
            .chain(&plan.having)
            .chain(&plan.group_by)
            .chain(&plan.distinct_on)
            .chain(plan.order.iter().map(|order| &order.expr))
        {
            fields.expression(expr, params);
        }
        for item in &plan.projection {
            match item {
                SelectItem::Function { function, .. } => fields.function(function, params),
                SelectItem::Expr { expr, .. } => fields.expression(expr, params),
                SelectItem::WindowFunction { function, .. } => {
                    for expr in function
                        .args
                        .iter()
                        .chain(function.filter.as_deref())
                        .chain(&function.partition_by)
                        .chain(function.order_by.iter().map(|order| &order.expr))
                    {
                        fields.expression(expr, params);
                    }
                }
                _ => {}
            }
        }
        fields
    }

    pub(in crate::executor::execution) fn needs_corpus(&self) -> bool {
        !self.scored.is_empty() || !self.expanded.is_empty()
    }

    fn expression(&mut self, expr: &Expr, params: &[Value]) {
        if let Expr::Function(call) = expr {
            self.function(call, params);
        } else {
            let _: std::ops::ControlFlow<()> = expr.try_for_each_child(|child| {
                self.expression(child, params);
                std::ops::ControlFlow::Continue(())
            });
        }
    }

    fn function(&mut self, call: &FunctionCall, params: &[Value]) {
        if let Some(Expr::Column(field)) = call.args.first() {
            if call.name.eq_ignore_ascii_case("snippet") {
                if snippet_needs_vocabulary(call, params) {
                    // Row lookup must distinguish identically named columns on either side of a join.
                    self.expanded.insert(field.clone());
                }
                self.snippets
                    .insert(crate::sql::ColumnIdentifierPath::reference_field_key(field));
            } else if call.name.eq_ignore_ascii_case("search")
                || call.name.eq_ignore_ascii_case("search_score")
            {
                self.scored
                    .insert(crate::sql::ColumnIdentifierPath::reference_field_key(field));
            }
        }
        for arg in call.expressions() {
            self.expression(arg, params);
        }
    }
}

fn text_argument<'a>(expr: &'a Expr, params: &'a [Value]) -> Option<&'a str> {
    match expr {
        Expr::StringLiteral(text) => Some(text),
        Expr::Param(index) => params.get(*index).and_then(Value::as_str),
        _ => None,
    }
}

fn snippet_needs_vocabulary(call: &FunctionCall, params: &[Value]) -> bool {
    let Some(syntax) = call.args.get(2) else {
        return false;
    };
    match text_argument(syntax, params) {
        Some("query") => call
            .args
            .get(1)
            .and_then(|expr| text_argument(expr, params))
            .is_none_or(cassie_search::query::requires_vocabulary),
        Some(_) => false,
        // A row-dependent selector or query may request expansions.
        None => true,
    }
}
