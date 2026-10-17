//! Bind a CTE's exported column names to borrowed producer expressions.
use crate::app::CassieError;
use crate::executor::retained_memory::add;
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{CommonTableExpression, Expr, QuerySource, SelectItem, SelectStatement};
use crate::sql::{parse_analysis, ColumnIdentifierPath};
use std::collections::HashMap;

pub(super) struct Input<'a> {
    bindings: HashMap<String, Value<'a>>,
    qualifier: Option<String>,
    name: &'a str,
}
enum Value<'a> {
    Item(&'a SelectItem),
    Column(String),
}
impl Value<'_> {
    fn expression(&self) -> Expr {
        match self {
            Self::Column(name) | Self::Item(SelectItem::Column { name, .. }) => {
                Expr::Column(name.clone())
            }
            Self::Item(SelectItem::Expr { expr, .. }) => expr.clone(),
            Self::Item(SelectItem::Function { function, .. }) => {
                Expr::Function(Box::new(function.clone()))
            }
            Self::Item(_) => unreachable!("flat CTE input"),
        }
    }
    fn bytes(&self, depth: usize, controls: &QueryExecutionControls) -> Result<usize, CassieError> {
        match self {
            Self::Item(item) => parse_analysis::item_clone_bytes(item, depth, controls),
            Self::Column(name) => add(size_of::<Expr>(), name.len()),
        }
    }
}

impl<'a> Input<'a> {
    pub(super) fn new(
        source: &QuerySource,
        cte: &'a CommonTableExpression,
        select: &'a SelectStatement,
        controls: &QueryExecutionControls,
    ) -> Result<Option<Self>, CassieError> {
        let qualifier = if let QuerySource::Aliased { alias, .. } = source {
            Some(crate::sql::binder::alias_row_qualifier(alias))
        } else {
            None
        };
        let mut bindings = HashMap::new();
        if matches!(select.projection.as_slice(), [SelectItem::Wildcard]) {
            let QuerySource::Values { rows } = &select.source else {
                return Ok(None);
            };
            let Some(first) = rows.first() else {
                return Ok(None);
            };
            for index in 0..first.len() {
                controls.check_interruption()?;
                let physical = format!("column{}", index + 1);
                let name = cte
                    .aliases
                    .get(index)
                    .filter(|alias| alias.as_str() != "*")
                    .cloned()
                    .unwrap_or_else(|| physical.clone());
                bindings.insert(
                    ColumnIdentifierPath::stored_field_key(&name),
                    Value::Column(physical),
                );
            }
        } else {
            for (index, item) in select.projection.iter().enumerate() {
                controls.check_interruption()?;
                let name = cte
                    .aliases
                    .get(index)
                    .filter(|alias| alias.as_str() != "*")
                    .cloned()
                    .unwrap_or_else(|| match item {
                        SelectItem::Column { name, alias } => alias.clone().unwrap_or_else(|| {
                            ColumnIdentifierPath::parse(name)
                                .map_or_else(|_| name.clone(), |path| path.implicit_output_label())
                        }),
                        SelectItem::Expr { alias, .. } => {
                            alias.clone().unwrap_or_else(|| "?column?".into())
                        }
                        SelectItem::Function { function, alias } => {
                            alias.clone().unwrap_or_else(|| function.name.clone())
                        }
                        _ => unreachable!("flat projection"),
                    });
                bindings.insert(
                    ColumnIdentifierPath::stored_field_key(&name),
                    Value::Item(item),
                );
            }
        }
        Ok(Some(Self {
            bindings,
            qualifier,
            name: &cte.name,
        }))
    }
    fn column(&self, name: &str) -> Option<&Value<'a>> {
        let path = ColumnIdentifierPath::parse(name).ok()?;
        if path.is_qualified() {
            let own = path.namespace_qualifier()?;
            if self.qualifier.as_deref() != Some(own.as_str())
                && !own.eq_ignore_ascii_case(self.name)
            {
                return None;
            }
        }
        let field = path.field_lookup_key();
        self.bindings.get(&field)
    }
    pub(super) fn estimate(
        &self,
        expr: &Expr,
        depth: usize,
        controls: &QueryExecutionControls,
    ) -> Result<usize, CassieError> {
        controls.check_interruption()?;
        if let Expr::Column(name) = expr {
            if let Some(value) = self.column(name) {
                return value.bytes(depth, controls);
            }
        }
        let mut bytes = parse_analysis::expression_clone_bytes(expr, depth, controls)?;
        expr.try_visit_children(|child| {
            bytes = add(bytes, self.estimate(child, depth + 1, controls)?)?;
            Ok::<_, CassieError>(())
        })?;
        Ok(bytes)
    }
    pub(super) fn rewrite(
        &self,
        expr: &Expr,
        controls: &QueryExecutionControls,
    ) -> Result<Expr, CassieError> {
        controls.check_interruption()?;
        if let Expr::Column(name) = expr {
            return self.column(name).map(Value::expression).ok_or_else(|| {
                CassieError::Unsupported(
                    "NOT MATERIALIZED outer reference expansion remains pending under #801/#800"
                        .into(),
                )
            });
        }
        if matches!(expr, Expr::Exists(_)) {
            return Err(CassieError::Unsupported(
                "NOT MATERIALIZED correlated consumer expansion remains pending under #801/#800"
                    .into(),
            ));
        }
        let mut failure = None;
        let result = expr.map_children(|child| match self.rewrite(child, controls) {
            Ok(value) => value,
            Err(error) => {
                failure = Some(error);
                Expr::Null
            }
        });
        failure.map_or(Ok(result), Err)
    }
    pub(super) fn item(
        &self,
        item: &SelectItem,
        controls: &QueryExecutionControls,
    ) -> Result<SelectItem, CassieError> {
        match item {
            SelectItem::Column { name, alias } => {
                let original = Expr::Column(name.clone());
                let expr = self.rewrite(&original, controls)?;
                let alias = alias.clone().or_else(|| {
                    ColumnIdentifierPath::parse(name)
                        .ok()
                        .map(|path| path.implicit_output_label())
                });
                Ok(SelectItem::Expr { expr, alias })
            }
            SelectItem::Expr { expr, alias } => Ok(SelectItem::Expr {
                expr: self.rewrite(expr, controls)?,
                alias: alias.clone(),
            }),
            SelectItem::Function { function, alias } => {
                let mut function = function.clone();
                for expr in function.expressions_mut() {
                    *expr = self.rewrite(expr, controls)?;
                }
                Ok(SelectItem::Function {
                    function,
                    alias: alias.clone(),
                })
            }
            _ => Err(CassieError::Unsupported(
                "NOT MATERIALIZED consumer wildcard/window expansion remains pending under #801"
                    .into(),
            )),
        }
    }
}
