//! Semantic analysis for wire Parse, without physical planning or constant folding.
//!
//! Keep catalog-sensitive calls intact for Describe and Execute. Irrevocable
//! UNKNOWN parameter type coercions are normalized with the declared Parse OIDs.
//! The temporary binding-analysis copy has its own query lease.
use crate::app::CassieError;
use crate::executor::retained_memory::{add, data_type_clone_bytes, mul};
use crate::runtime::QueryExecutionControls;
use crate::sql::ast::{
    CteQuery, Expr, FunctionCall, ParsedStatement, QuerySource, QueryStatement, SelectItem,
    SelectStatement,
};
use std::cell::Cell;

#[path = "parse_analysis/type_functions.rs"]
mod type_functions;
pub(crate) use type_functions::normalize as normalize_type_functions;

pub(crate) fn validate(
    parsed: &ParsedStatement,
    parameter_types: &[i32],
    catalog: &crate::catalog::Catalog,
    context: &super::binder::BindingContext,
    controls: &QueryExecutionControls,
) -> Result<(), CassieError> {
    if !is_select(parsed, controls)? {
        return Ok(());
    }
    let bytes = Footprint::new(controls).statement(parsed, 0)?;
    let _analysis_memory = controls.reserve_query_memory(bytes)?;
    controls.check_interruption()?;
    let mut analysis = parsed.clone();
    super::binder::values::bind_input_types(&mut analysis, parameter_types)?;
    super::binder::coerce_parameterized_boolean_case_results(&mut analysis, parameter_types);
    let mut bound =
        super::binder::bind_initial_with_controls(analysis, catalog, context, Some(controls))?;
    super::binder::bind_boolean_parameters(
        &mut bound.statement,
        catalog,
        context,
        parameter_types,
    )?;
    controls.check_interruption()
}

fn is_select(
    parsed: &ParsedStatement,
    controls: &QueryExecutionControls,
) -> Result<bool, CassieError> {
    let mut current = parsed;
    let mut depth = 0;
    loop {
        Footprint::new(controls).check(depth)?;
        depth += 1;
        match &current.statement {
            QueryStatement::Select(_) => return Ok(true),
            QueryStatement::Explain(explain) => current = &explain.statement,
            _ => return Ok(false),
        }
    }
}

/// Analyze a known rejected nested input at its ordered binding position.
/// The temporary tree retires inside `validate` before its lease is released.
pub(super) fn validate_rejected_copy(
    parsed: &ParsedStatement,
    controls: Option<&QueryExecutionControls>,
    validate: impl FnOnce(ParsedStatement) -> Result<(), CassieError>,
) -> Result<(), CassieError> {
    let footprint = Footprint::optional(controls, true);
    let bytes = footprint.statement(parsed, 0)?;
    if !footprint.rejected_cast.get() {
        return Ok(());
    }
    let _analysis_memory = controls
        .map(|controls| controls.reserve_query_memory(bytes))
        .transpose()?;
    if let Some(controls) = controls {
        controls.check_interruption()?;
    }
    validate(parsed.clone())
}

struct Footprint<'a> {
    controls: Option<&'a QueryExecutionControls>,
    rejected_cast: Cell<bool>,
    detect_rejected: bool,
}

impl<'a> Footprint<'a> {
    const fn new(controls: &'a QueryExecutionControls) -> Self {
        Self::optional(Some(controls), false)
    }

    const fn optional(controls: Option<&'a QueryExecutionControls>, detect_rejected: bool) -> Self {
        Self {
            controls,
            rejected_cast: Cell::new(false),
            detect_rejected,
        }
    }

    fn check(&self, depth: usize) -> Result<(), CassieError> {
        if let Some(controls) = self.controls {
            controls.check_interruption()?;
        }
        if depth > 128 {
            return Err(CassieError::ResourceLimit(
                "Parse analysis nesting exceeds 128 levels".into(),
            ));
        }
        Ok(())
    }

    fn statement(&self, parsed: &ParsedStatement, depth: usize) -> Result<usize, CassieError> {
        self.check(depth)?;
        let bytes = add(size_of::<ParsedStatement>(), parsed.raw_sql.len())?;
        let children = match &parsed.statement {
            QueryStatement::Select(select) => self.select(select, depth + 1)?,
            QueryStatement::Explain(explain) => self.statement(&explain.statement, depth + 1)?,
            QueryStatement::Insert(_)
            | QueryStatement::Update(_)
            | QueryStatement::Delete(_)
            | QueryStatement::Merge(_) => {
                // A data-modifying CTE body retains its encoded statement.
                serde_json::to_vec(&parsed.statement)
                    .map(|encoded| encoded.len())
                    .map_err(|error| CassieError::ResourceLimit(error.to_string()))?
            }
            _ => {
                return Err(CassieError::Unsupported(
                    "non-SELECT CTE Parse analysis is not yet supported".into(),
                ));
            }
        };
        add(bytes, children)
    }

    fn strings(values: &[String]) -> Result<usize, CassieError> {
        values
            .iter()
            .try_fold(mul(values.len(), size_of::<String>())?, |bytes, value| {
                add(bytes, value.len())
            })
    }

    fn expressions(&self, values: &[Expr], depth: usize) -> Result<usize, CassieError> {
        values.iter().try_fold(0, |bytes, expr| {
            add(bytes, self.expression(expr, depth + 1)?)
        })
    }

    fn function(&self, function: &FunctionCall, depth: usize) -> Result<usize, CassieError> {
        self.check(depth)?;
        let mut bytes = add(size_of::<FunctionCall>(), function.name.len())?;
        bytes = add(bytes, self.expressions(&function.args, depth + 1)?)?;
        if let Some(filter) = &function.filter {
            bytes = add(bytes, self.expression(filter, depth + 1)?)?;
        }
        for order in &function.order_by {
            bytes = add(bytes, size_of::<crate::sql::ast::OrderExpr>())?;
            bytes = add(bytes, self.expression(&order.expr, depth + 1)?)?;
        }
        Ok(bytes)
    }

    fn expression(&self, expr: &Expr, depth: usize) -> Result<usize, CassieError> {
        self.check(depth)?;
        if self.detect_rejected && super::literal_inputs::rejects_cast(expr) {
            self.rejected_cast.set(true);
        }
        let mut bytes = size_of::<Expr>();
        match expr {
            Expr::Column(text) | Expr::StringLiteral(text) | Expr::NumericLiteral(text) => {
                bytes = add(bytes, text.len())?;
            }
            Expr::Cast {
                data_type,
                source_type,
                ..
            } => {
                bytes = add(bytes, data_type_clone_bytes(data_type)?)?;
                if let Some(source) = source_type {
                    bytes = add(bytes, data_type_clone_bytes(source)?)?;
                }
            }
            Expr::Case { branches, .. } => {
                bytes = add(bytes, mul(branches.len(), size_of::<(Expr, Expr)>())?)?;
            }
            Expr::RowCompare(_) => {
                bytes = add(bytes, size_of::<crate::sql::ast::RowComparison>())?;
            }
            Expr::Function(function) => return add(bytes, self.function(function, depth + 1)?),
            Expr::Exists(statement) => return add(bytes, self.statement(statement, depth + 1)?),
            Expr::Subquery(subquery) => {
                bytes = add(bytes, size_of::<crate::sql::ast::SubqueryExpr>())?;
                bytes = add(bytes, self.statement(&subquery.query, depth + 1)?)?;
            }
            _ => {}
        }
        expr.try_visit_children(|child| {
            bytes = add(bytes, self.expression(child, depth + 1)?)?;
            Ok::<_, CassieError>(())
        })?;
        Ok(bytes)
    }

    fn source(&self, source: &QuerySource, depth: usize) -> Result<usize, CassieError> {
        self.check(depth)?;
        let mut bytes = size_of::<QuerySource>();
        match source {
            QuerySource::SingleRow => {}
            QuerySource::Collection(path) => {
                bytes = add(bytes, path.len())?;
                bytes = add(
                    bytes,
                    mul(
                        path.components().len(),
                        size_of::<crate::sql::ast::IdentifierComponent>(),
                    )?,
                )?;
                for component in path.components() {
                    bytes = add(bytes, component.value().len())?;
                }
            }
            QuerySource::Cte(name) => bytes = add(bytes, name.len())?,
            QuerySource::Values { rows } => {
                bytes = add(bytes, mul(rows.len(), size_of::<Vec<Expr>>())?)?;
                for row in rows {
                    bytes = add(bytes, self.expressions(row, depth + 1)?)?;
                }
            }
            QuerySource::Aliased {
                source,
                alias,
                column_aliases,
            } => {
                bytes = add(bytes, self.source(source, depth + 1)?)?;
                bytes = add(bytes, alias.len())?;
                bytes = add(bytes, Self::strings(column_aliases)?)?;
            }
            QuerySource::TableFunction { name, function, .. } => {
                bytes = add(bytes, name.len())?;
                bytes = add(bytes, self.function(function, depth + 1)?)?;
            }
            QuerySource::Subquery { alias, select, .. } => {
                bytes = add(bytes, alias.len())?;
                bytes = add(bytes, self.select(select, depth + 1)?)?;
            }
            QuerySource::Join {
                left, right, on, ..
            } => {
                bytes = add(bytes, self.source(left, depth + 1)?)?;
                bytes = add(bytes, self.source(right, depth + 1)?)?;
                bytes = add(bytes, self.expression(on, depth + 1)?)?;
            }
        }
        Ok(bytes)
    }

    fn cte(
        &self,
        cte: &crate::sql::ast::CommonTableExpression,
        depth: usize,
    ) -> Result<usize, CassieError> {
        self.check(depth)?;
        let mut bytes = add(
            size_of::<crate::sql::ast::CommonTableExpression>(),
            cte.name.len(),
        )?;
        bytes = add(bytes, Self::strings(&cte.aliases)?)?;
        match &cte.query {
            CteQuery::Simple(statement) => {
                bytes = add(bytes, self.statement(statement, depth + 1)?)?;
            }
            CteQuery::Recursive {
                base, recursive, ..
            } => {
                bytes = add(bytes, self.statement(base, depth + 1)?)?;
                bytes = add(bytes, self.statement(recursive, depth + 1)?)?;
            }
        }
        Ok(bytes)
    }

    fn item(&self, item: &SelectItem, depth: usize) -> Result<usize, CassieError> {
        self.check(depth)?;
        let mut bytes = size_of::<SelectItem>();
        let alias = match item {
            SelectItem::Wildcard => None,
            SelectItem::Column { name, alias } => {
                bytes = add(bytes, name.len())?;
                alias.as_ref()
            }
            SelectItem::Expr { expr, alias } => {
                bytes = add(bytes, self.expression(expr, depth + 1)?)?;
                alias.as_ref()
            }
            SelectItem::Function { function, alias } => {
                bytes = add(bytes, self.function(function, depth + 1)?)?;
                alias.as_ref()
            }
            SelectItem::WindowFunction { function, alias } => {
                bytes = add(bytes, function.name.len())?;
                bytes = add(bytes, self.expressions(&function.args, depth + 1)?)?;
                bytes = add(bytes, self.expressions(&function.partition_by, depth + 1)?)?;
                for order in &function.order_by {
                    bytes = add(bytes, size_of::<crate::sql::ast::OrderExpr>())?;
                    bytes = add(bytes, self.expression(&order.expr, depth + 1)?)?;
                }
                alias.as_ref()
            }
        };
        if let Some(alias) = alias {
            bytes = add(bytes, alias.len())?;
        }
        Ok(bytes)
    }

    fn select(&self, select: &SelectStatement, depth: usize) -> Result<usize, CassieError> {
        self.check(depth)?;
        let mut bytes = add(
            size_of::<SelectStatement>(),
            self.source(&select.source, depth + 1)?,
        )?;
        for cte in &select.ctes {
            bytes = add(bytes, self.cte(cte, depth)?)?;
        }
        for item in &select.projection {
            bytes = add(bytes, self.item(item, depth)?)?;
        }
        bytes = add(bytes, self.expressions(&select.distinct_on, depth + 1)?)?;
        bytes = add(bytes, self.expressions(&select.group_by, depth + 1)?)?;
        for expr in [
            &select.filter,
            &select.having,
            &select.limit,
            &select.offset,
        ]
        .into_iter()
        .flatten()
        {
            bytes = add(bytes, self.expression(expr, depth + 1)?)?;
        }
        for order in &select.order {
            bytes = add(bytes, size_of::<crate::sql::ast::OrderExpr>())?;
            bytes = add(bytes, self.expression(&order.expr, depth + 1)?)?;
        }
        if let Some(set) = &select.set {
            bytes = add(bytes, size_of::<crate::sql::ast::SelectSet>())?;
            bytes = add(bytes, self.select(&set.right, depth + 1)?)?;
        }
        Ok(bytes)
    }
}

// Shared controlled AST clone accounting for the CTE planner's temporary copy.
pub(crate) fn source_clone_bytes(
    source: &QuerySource,
    controls: &QueryExecutionControls,
) -> Result<usize, CassieError> {
    Footprint::new(controls).source(source, 0)
}
pub(crate) fn cte_clone_bytes(
    cte: &crate::sql::ast::CommonTableExpression,
    controls: &QueryExecutionControls,
) -> Result<usize, CassieError> {
    Footprint::new(controls).cte(cte, 0)
}
pub(crate) fn item_clone_bytes(
    item: &SelectItem,
    depth: usize,
    controls: &QueryExecutionControls,
) -> Result<usize, CassieError> {
    Footprint::new(controls).item(item, depth)
}
pub(crate) fn expression_clone_bytes(
    expr: &Expr,
    depth: usize,
    controls: &QueryExecutionControls,
) -> Result<usize, CassieError> {
    Footprint::new(controls).expression(expr, depth)
}
pub(crate) fn select_clone_bytes(
    select: &SelectStatement,
    controls: &QueryExecutionControls,
) -> Result<usize, CassieError> {
    Footprint::new(controls).select(select, 0)
}
