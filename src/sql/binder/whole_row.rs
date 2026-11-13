//! Whole-row references and `(expr).*` expansion (#828).
//!
//! Every table owns a row type named like the table. For a `SELECT` over one
//! table, a bare reference to the table's name (or alias) stands for the whole
//! row, `ROW(t.*)` expands to the table's columns and `(expr).*` expands to the
//! fields of a composite column or cast in the SELECT list. The pass rewrites
//! those forms into ordinary `ROW(...)` casts and field selections before
//! binding, so the rest of the engine only sees constructs it already handles.
use super::{BindingContext, CassieError, Catalog, Expr, QuerySource, SelectItem, SelectStatement};
use crate::sql::ast::{
    CastIdentity, FunctionCall, ParsedStatement, QueryStatement, FIELD_ACCESS_ALL,
    FIELD_ACCESS_FUNCTION, ROW_CONSTRUCTOR_FUNCTION,
};
use crate::types::composite::CompositeRef;
use crate::types::DataType;

/// The table a single-table SELECT reads and the name its row is referenced by.
struct RowScope {
    binding: String,
    row_type: CompositeRef,
    schema_types: Vec<(String, DataType)>,
}

fn scope_for(
    select: &SelectStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Option<RowScope> {
    let (path, alias) = match &select.source {
        QuerySource::Collection(path) => (path, None),
        QuerySource::Aliased { source, alias, .. } => match source.as_ref() {
            QuerySource::Collection(path) => (path, Some(alias.as_str())),
            _ => return None,
        },
        _ => return None,
    };
    let canonical = super::resolve_relation_name(path.as_str(), catalog, context).ok()?;
    let row_type = catalog.relation_row_type(&canonical)?;
    let schema = catalog.get_schema(&canonical)?;
    let binding = alias.map_or_else(|| crate::catalog::local_name(&canonical), str::to_owned);
    Some(RowScope {
        binding: binding.to_ascii_lowercase(),
        row_type,
        schema_types: schema
            .fields
            .into_iter()
            .map(|field| (field.name, field.data_type))
            .collect(),
    })
}

fn row_constructor(args: Vec<Expr>) -> Expr {
    Expr::Function(Box::new(FunctionCall {
        name: ROW_CONSTRUCTOR_FUNCTION.into(),
        args,
        ..FunctionCall::default()
    }))
}

impl RowScope {
    /// A column called like the table makes the bare name a column reference.
    fn names_a_column(&self) -> bool {
        self.row_type
            .fields
            .iter()
            .any(|field| field.name.eq_ignore_ascii_case(&self.binding))
    }

    fn columns(&self) -> Vec<Expr> {
        self.row_type
            .fields
            .iter()
            .map(|field| Expr::Column(field.name.clone()))
            .collect()
    }

    fn whole_row(&self) -> Expr {
        Expr::Cast {
            expr: Box::new(row_constructor(self.columns())),
            data_type: DataType::Composite(self.row_type.clone()),
            source_type: None,
            identity: CastIdentity::Scalar,
        }
    }

    fn is_whole_row_name(&self, name: &str) -> bool {
        !self.names_a_column() && name.eq_ignore_ascii_case(&self.binding)
    }

    fn is_star_of_row(&self, name: &str) -> bool {
        name.strip_suffix(".*")
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(&self.binding))
    }

    /// The composite type `expr` evaluates to, when it is evident from the schema.
    fn composite_of(&self, expr: &Expr) -> Option<CompositeRef> {
        match expr {
            Expr::Column(name) if self.is_whole_row_name(name) => Some(self.row_type.clone()),
            Expr::Column(name) => self
                .schema_types
                .iter()
                .find(|(field, _)| field.eq_ignore_ascii_case(name))
                .and_then(|(_, data_type)| match data_type {
                    DataType::Composite(descriptor) => Some(descriptor.clone()),
                    _ => None,
                }),
            Expr::Cast {
                data_type: DataType::Composite(descriptor),
                ..
            } => Some(descriptor.clone()),
            Expr::Function(function)
                if function.name.eq_ignore_ascii_case(FIELD_ACCESS_FUNCTION) =>
            {
                let [inner, Expr::StringLiteral(field)] = function.args.as_slice() else {
                    return None;
                };
                let owner = self.composite_of(inner)?;
                let index = owner.field_index(field)?;
                match &owner.fields[index].data_type {
                    DataType::Composite(descriptor) => Some(descriptor.clone()),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Replaces whole-row names and expands `ROW(t.*)` inside `expr`.
    fn rewrite_expr(&self, expr: &mut Expr) {
        match expr {
            Expr::Column(name) if self.is_whole_row_name(name) => {
                *expr = self.whole_row();
            }
            Expr::Function(function)
                if function.name.eq_ignore_ascii_case(ROW_CONSTRUCTOR_FUNCTION) =>
            {
                let mut args = Vec::with_capacity(function.args.len());
                for mut arg in std::mem::take(&mut function.args) {
                    match &arg {
                        Expr::Column(name) if self.is_star_of_row(name) => {
                            args.extend(self.columns());
                        }
                        _ => {
                            self.rewrite_expr(&mut arg);
                            args.push(arg);
                        }
                    }
                }
                function.args = args;
            }
            other => {
                let _ = other.try_visit_children_mut::<()>(|child| {
                    self.rewrite_expr(child);
                    Ok(())
                });
            }
        }
    }

    /// Expands `(expr).*` into one field selection per field of its type.
    fn expand_all_fields(&self, expr: &Expr) -> Result<Option<Vec<SelectItem>>, CassieError> {
        let Expr::Function(function) = expr else {
            return Ok(None);
        };
        if !function.name.eq_ignore_ascii_case(FIELD_ACCESS_FUNCTION) {
            return Ok(None);
        }
        let [inner, Expr::StringLiteral(field)] = function.args.as_slice() else {
            return Ok(None);
        };
        if field != FIELD_ACCESS_ALL {
            return Ok(None);
        }
        let Some(descriptor) = self.composite_of(inner) else {
            return Err(crate::app::DiagnosticError::new(
                "0A000",
                "(expression).* expansion is supported only for a column, table row or cast of a composite type"
                    .to_string(),
            )
            .into());
        };
        let mut inner = inner.clone();
        self.rewrite_expr(&mut inner);
        Ok(Some(
            descriptor
                .fields
                .iter()
                .map(|field| SelectItem::Expr {
                    expr: Expr::Function(Box::new(FunctionCall {
                        name: FIELD_ACCESS_FUNCTION.into(),
                        args: vec![inner.clone(), Expr::StringLiteral(field.name.clone())],
                        ..FunctionCall::default()
                    })),
                    alias: Some(field.name.clone()),
                })
                .collect(),
        ))
    }

    fn rewrite_item(&self, item: SelectItem) -> Result<Vec<SelectItem>, CassieError> {
        match item {
            SelectItem::Column { name, alias } if self.is_whole_row_name(&name) => {
                Ok(vec![SelectItem::Expr {
                    expr: self.whole_row(),
                    alias: Some(alias.unwrap_or(name)),
                }])
            }
            SelectItem::Function { function, alias } => {
                let expr = Expr::Function(Box::new(function.clone()));
                if let Some(items) = self.expand_all_fields(&expr)? {
                    return Ok(items);
                }
                let mut rewritten = expr.clone();
                self.rewrite_expr(&mut rewritten);
                Ok(vec![match rewritten {
                    Expr::Function(changed) if !same_call(&changed, &function) => {
                        SelectItem::Function {
                            function: *changed,
                            alias,
                        }
                    }
                    Expr::Function(_) => SelectItem::Function { function, alias },
                    other => SelectItem::Expr { expr: other, alias },
                }])
            }
            SelectItem::Expr { mut expr, alias } => {
                if let Some(items) = self.expand_all_fields(&expr)? {
                    return Ok(items);
                }
                self.rewrite_expr(&mut expr);
                Ok(vec![SelectItem::Expr { expr, alias }])
            }
            other => Ok(vec![other]),
        }
    }
}

fn same_call(left: &FunctionCall, right: &FunctionCall) -> bool {
    Expr::Function(Box::new(left.clone())).structurally_eq(&Expr::Function(Box::new(right.clone())))
}

/// Rewrites whole-row forms of a top-level single-table `SELECT`.
///
/// # Errors
///
/// Returns SQLSTATE `0A000` for a `(expr).*` whose type is not evident.
pub(super) fn rewrite(
    statement: &mut ParsedStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    let QueryStatement::Select(select) = &mut statement.statement else {
        return Ok(());
    };
    let Some(scope) = scope_for(select, catalog, context) else {
        return Ok(());
    };
    let items = std::mem::take(&mut select.projection);
    let mut rewritten = Vec::with_capacity(items.len());
    for item in items {
        rewritten.extend(scope.rewrite_item(item)?);
    }
    select.projection = rewritten;
    if let Some(filter) = &mut select.filter {
        scope.rewrite_expr(filter);
    }
    for order in &mut select.order {
        scope.rewrite_expr(&mut order.expr);
    }
    Ok(())
}
