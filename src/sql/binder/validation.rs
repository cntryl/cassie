use super::*;

pub(super) fn select_contains_parameters(select: &SelectStatement) -> bool {
    select.ctes.iter().any(cte_contains_parameters)
        || source_contains_parameters(&select.source)
        || select
            .projection
            .iter()
            .any(select_item_contains_parameters)
        || select.filter.as_ref().is_some_and(expr_contains_parameters)
        || select.distinct_on.iter().any(expr_contains_parameters)
        || select.group_by.iter().any(expr_contains_parameters)
        || select.having.as_ref().is_some_and(expr_contains_parameters)
        || select
            .order
            .iter()
            .any(|order| expr_contains_parameters(&order.expr))
        || select
            .set
            .as_ref()
            .is_some_and(|set| select_contains_parameters(&set.right))
}

pub(super) fn cte_contains_parameters(cte: &CommonTableExpression) -> bool {
    match &cte.query {
        CteQuery::Simple(statement) => parsed_statement_contains_parameters(statement.as_ref()),
        CteQuery::Recursive { base, recursive } => {
            parsed_statement_contains_parameters(base.as_ref())
                || parsed_statement_contains_parameters(recursive.as_ref())
        }
    }
}

pub(super) fn parsed_statement_contains_parameters(statement: &ParsedStatement) -> bool {
    match &statement.statement {
        QueryStatement::Explain(statement) => {
            parsed_statement_contains_parameters(statement.statement.as_ref())
        }
        QueryStatement::Select(select) => select_contains_parameters(select),
        QueryStatement::Show(_)
        | QueryStatement::Set(_)
        | QueryStatement::Insert(_)
        | QueryStatement::Update(_)
        | QueryStatement::Delete(_)
        | QueryStatement::Transaction(_)
        | QueryStatement::CreateTable(_)
        | QueryStatement::DropTable(_)
        | QueryStatement::AlterTable(_)
        | QueryStatement::CreateSchema(_)
        | QueryStatement::CreateView(_)
        | QueryStatement::DropView(_)
        | QueryStatement::CreateRole(_)
        | QueryStatement::AlterRole(_)
        | QueryStatement::DropRole(_)
        | QueryStatement::CreateIndex(_)
        | QueryStatement::DropIndex(_)
        | QueryStatement::DropSchema(_)
        | QueryStatement::AlterSchema(_)
        | QueryStatement::CreateFunction(_)
        | QueryStatement::DropFunction(_)
        | QueryStatement::CreateProcedure(_)
        | QueryStatement::DropProcedure(_)
        | QueryStatement::CallProcedure(_)
        | QueryStatement::CreateRollup(_)
        | QueryStatement::RefreshRollup(_)
        | QueryStatement::DropRollup(_)
        | QueryStatement::CreateRetentionPolicy(_)
        | QueryStatement::AlterRetentionPolicy(_)
        | QueryStatement::DropRetentionPolicy(_)
        | QueryStatement::EnforceRetentionPolicy(_) => false,
    }
}

pub(super) fn select_item_contains_parameters(item: &SelectItem) -> bool {
    match item {
        SelectItem::Wildcard => false,
        SelectItem::Column { .. } => false,
        SelectItem::Function { function, .. } => function.args.iter().any(expr_contains_parameters),
        SelectItem::Expr { expr, .. } => expr_contains_parameters(expr),
        SelectItem::WindowFunction { function, .. } => {
            function.args.iter().any(expr_contains_parameters)
                || function.partition_by.iter().any(expr_contains_parameters)
                || function
                    .order_by
                    .iter()
                    .any(|order| expr_contains_parameters(&order.expr))
        }
    }
}

pub(super) fn source_contains_parameters(source: &QuerySource) -> bool {
    match source {
        QuerySource::Collection(_) | QuerySource::Cte(_) | QuerySource::SingleRow => false,
        QuerySource::Subquery { select, .. } => select_contains_parameters(select),
        QuerySource::Join {
            left, right, on, ..
        } => {
            source_contains_parameters(left)
                || source_contains_parameters(right)
                || expr_contains_parameters(on)
        }
    }
}

pub(super) fn expr_contains_parameters(expr: &Expr) -> bool {
    match expr {
        Expr::Param(_) => true,
        Expr::Binary { left, right, .. } => {
            expr_contains_parameters(left) || expr_contains_parameters(right)
        }
        Expr::IsNull { expr, .. } | Expr::Cast { expr, .. } => expr_contains_parameters(expr),
        Expr::InList { expr, values, .. } => {
            expr_contains_parameters(expr) || values.iter().any(expr_contains_parameters)
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            expr_contains_parameters(expr)
                || expr_contains_parameters(low)
                || expr_contains_parameters(high)
        }
        Expr::Not { expr } => expr_contains_parameters(expr),
        Expr::Exists(statement) => parsed_statement_contains_parameters(statement),
        Expr::Function(function) => function.args.iter().any(expr_contains_parameters),
        Expr::Column(_)
        | Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null => false,
    }
}

pub(super) fn qualified_fields(
    qualifier: &str,
    fields: impl IntoIterator<Item = String>,
) -> HashSet<String> {
    let qualifier = qualifier.to_ascii_lowercase();
    let mut out = HashSet::new();
    for field in fields {
        let field = field.to_ascii_lowercase();
        out.insert(field.clone());
        out.insert(format!("{qualifier}.{field}"));
    }
    out
}

pub(super) fn collect_projection_aliases(select: &SelectStatement) -> HashSet<String> {
    let mut aliases = HashSet::new();
    for item in &select.projection {
        match item {
            SelectItem::Column {
                alias: Some(alias), ..
            }
            | SelectItem::Function {
                alias: Some(alias), ..
            }
            | SelectItem::WindowFunction {
                alias: Some(alias), ..
            } => {
                aliases.insert(alias.to_ascii_lowercase());
            }
            _ => {}
        }
    }
    aliases
}

pub(super) fn validate_functions(
    statement: &SelectStatement,
    catalog: &Catalog,
) -> Result<(), CassieError> {
    let mut seen = Vec::new();
    collect_functions(statement, &mut seen);
    validate_function_calls(seen, catalog)
}

pub(super) fn validate_function_calls(
    functions: Vec<FunctionCall>,
    catalog: &Catalog,
) -> Result<(), CassieError> {
    let mut signatures = crate::sql::functions::registry()
        .into_iter()
        .map(|function| (function.name.to_ascii_lowercase(), function.arity))
        .collect::<HashMap<_, _>>();

    for function in catalog.list_functions() {
        signatures.insert(
            function.name.to_ascii_lowercase(),
            crate::sql::functions::FunctionArity::Exact(function.args.len()),
        );
    }

    for function in functions {
        if function.name.eq_ignore_ascii_case("cast") {
            if function.args.len() != 2 {
                return Err(CassieError::Planner(format!(
                    "function '{}' expects 2 args",
                    function.name
                )));
            }
            continue;
        }
        if let Some(arity) = crate::sql::functions::aggregate_arity(&function.name) {
            if function.args.len() != arity {
                return Err(CassieError::Planner(format!(
                    "aggregate function '{}' expects {} args, got {}",
                    function.name,
                    arity,
                    function.args.len()
                )));
            }
            continue;
        }
        let Some(arity) = signatures.get(&function.name.to_ascii_lowercase()) else {
            return Err(CassieError::Planner(format!(
                "unsupported function '{}'",
                function.name
            )));
        };
        if !arity.matches(function.args.len()) {
            return Err(CassieError::Planner(format!(
                "function '{}' expects {}, got {}",
                function.name,
                arity.describe(),
                function.args.len()
            )));
        }
    }

    Ok(())
}

pub(super) fn validate_projection_references(
    projection: &[SelectItem],
    known_fields: &HashSet<String>,
) -> Result<(), CassieError> {
    for item in projection {
        match item {
            SelectItem::Wildcard => {}
            SelectItem::Column { name, .. } => {
                validate_expression(
                    &Expr::Column(name.clone()),
                    known_fields,
                    &HashSet::new(),
                    false,
                )?;
            }
            SelectItem::Function { function, .. } => {
                if crate::sql::functions::is_aggregate_function(&function.name) {
                    validate_aggregate_function_args(function, known_fields)?;
                    continue;
                }
                for arg in &function.args {
                    validate_expression(arg, known_fields, &HashSet::new(), false)?;
                }
            }
            SelectItem::Expr { expr, .. } => {
                validate_expression(expr, known_fields, &HashSet::new(), false)?;
            }
            SelectItem::WindowFunction { function, .. } => {
                for arg in &function.args {
                    validate_expression(arg, known_fields, &HashSet::new(), false)?;
                }
                for expr in &function.partition_by {
                    validate_expression(expr, known_fields, &HashSet::new(), false)?;
                }
                for order in &function.order_by {
                    validate_expression(&order.expr, known_fields, &HashSet::new(), false)?;
                }
            }
        }
    }
    Ok(())
}

pub(super) fn validate_expression_references(
    expression: Option<&Expr>,
    known_fields: &HashSet<String>,
    projection_aliases: &HashSet<String>,
    allow_projection_alias: bool,
) -> Result<(), CassieError> {
    let Some(expression) = expression else {
        return Ok(());
    };
    validate_expression(
        expression,
        known_fields,
        projection_aliases,
        allow_projection_alias,
    )
}

pub(super) fn validate_order_by_references(
    order: &[crate::sql::ast::OrderExpr],
    known_fields: &HashSet<String>,
    projection_aliases: &HashSet<String>,
) -> Result<(), CassieError> {
    for item in order {
        validate_expression(&item.expr, known_fields, projection_aliases, true)?;
    }
    Ok(())
}

pub(super) fn validate_distinct_on_order_prefix(
    distinct_on: &[Expr],
    order: &[OrderExpr],
) -> Result<(), CassieError> {
    if distinct_on.is_empty() {
        return Ok(());
    }
    if order.len() < distinct_on.len() {
        return Err(CassieError::Planner(
            "DISTINCT ON expressions must match the leading ORDER BY expressions".to_string(),
        ));
    }
    for (distinct_expr, order_expr) in distinct_on.iter().zip(order.iter()) {
        if !distinct_on_expr_matches_order(distinct_expr, &order_expr.expr) {
            return Err(CassieError::Planner(
                "DISTINCT ON expressions must match the leading ORDER BY expressions".to_string(),
            ));
        }
    }
    Ok(())
}

pub(super) fn distinct_on_expr_matches_order(left: &Expr, right: &Expr) -> bool {
    match (left, right) {
        (Expr::Column(left), Expr::Column(right)) => left.eq_ignore_ascii_case(right),
        _ => format!("{left:?}") == format!("{right:?}"),
    }
}

pub(super) fn validate_expression(
    expr: &Expr,
    known_fields: &HashSet<String>,
    projection_aliases: &HashSet<String>,
    allow_projection_alias: bool,
) -> Result<(), CassieError> {
    match expr {
        Expr::Column(name) => {
            let name = name.to_ascii_lowercase();
            if known_fields.contains("*") || name == "id" || known_fields.contains(&name) {
                return Ok(());
            }

            if allow_projection_alias && projection_aliases.contains(&name) {
                return Ok(());
            }

            Err(CassieError::Planner(format!(
                "unresolvable column reference '{}'; known fields or aliases required",
                name
            )))
        }
        Expr::Binary { left, right, .. } => {
            validate_expression(
                left,
                known_fields,
                projection_aliases,
                allow_projection_alias,
            )?;
            validate_expression(
                right,
                known_fields,
                projection_aliases,
                allow_projection_alias,
            )
        }
        Expr::IsNull { expr, .. } => validate_expression(
            expr,
            known_fields,
            projection_aliases,
            allow_projection_alias,
        ),
        Expr::InList { expr, values, .. } => {
            validate_expression(
                expr,
                known_fields,
                projection_aliases,
                allow_projection_alias,
            )?;
            for value in values {
                validate_expression(
                    value,
                    known_fields,
                    projection_aliases,
                    allow_projection_alias,
                )?;
            }
            Ok(())
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            validate_expression(
                expr,
                known_fields,
                projection_aliases,
                allow_projection_alias,
            )?;
            validate_expression(
                low,
                known_fields,
                projection_aliases,
                allow_projection_alias,
            )?;
            validate_expression(
                high,
                known_fields,
                projection_aliases,
                allow_projection_alias,
            )
        }
        Expr::Not { expr } => validate_expression(
            expr,
            known_fields,
            projection_aliases,
            allow_projection_alias,
        ),
        Expr::Cast { expr, .. } => validate_expression(
            expr,
            known_fields,
            projection_aliases,
            allow_projection_alias,
        ),
        Expr::Exists(_) => Ok(()),
        Expr::Function(function) => {
            if crate::sql::functions::is_aggregate_function(&function.name) {
                validate_aggregate_function_args(function, known_fields)?;
                return Ok(());
            }
            for arg in &function.args {
                validate_expression(
                    arg,
                    known_fields,
                    projection_aliases,
                    allow_projection_alias,
                )?;
            }
            Ok(())
        }
        Expr::Param(_)
        | Expr::Null
        | Expr::BoolLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::StringLiteral(_) => Ok(()),
    }
}

pub(super) fn validate_aggregate_function_args(
    function: &FunctionCall,
    known_fields: &HashSet<String>,
) -> Result<(), CassieError> {
    let Some(arity) = crate::sql::functions::aggregate_arity(&function.name) else {
        return Ok(());
    };
    if function.args.len() != arity {
        return Err(CassieError::Planner(format!(
            "aggregate function '{}' expects {} args, got {}",
            function.name,
            arity,
            function.args.len()
        )));
    }
    if function.name.eq_ignore_ascii_case("count")
        && matches!(function.args.as_slice(), [Expr::Column(name)] if name == "*")
    {
        return Ok(());
    }
    for arg in &function.args {
        validate_expression(arg, known_fields, &HashSet::new(), false)?;
    }
    Ok(())
}

pub(super) fn collect_functions(statement: &SelectStatement, out: &mut Vec<FunctionCall>) {
    for item in &statement.projection {
        collect_item(item, out);
    }
    if let Some(expr) = &statement.filter {
        collect_expr(expr, out);
    }
    if let Some(expr) = &statement.having {
        collect_expr(expr, out);
    }
    for expr in &statement.distinct_on {
        collect_expr(expr, out);
    }
    for expr in &statement.group_by {
        collect_expr(expr, out);
    }
    for order in &statement.order {
        collect_expr(&order.expr, out);
    }
    if let Some(set) = &statement.set {
        collect_functions(&set.right, out);
    }
    for cte in &statement.ctes {
        match &cte.query {
            CteQuery::Simple(statement) => {
                if let QueryStatement::Select(select) = &statement.statement {
                    collect_functions(select, out);
                }
            }
            CteQuery::Recursive { base, recursive } => {
                if let QueryStatement::Select(select) = &base.statement {
                    collect_functions(select, out);
                }
                if let QueryStatement::Select(select) = &recursive.statement {
                    collect_functions(select, out);
                }
            }
        }
    }
}

pub(super) fn collect_item(item: &SelectItem, out: &mut Vec<FunctionCall>) {
    match item {
        SelectItem::Function { function, .. } => {
            out.push(function.clone());
            for arg in &function.args {
                collect_expr(arg, out);
            }
        }
        SelectItem::WindowFunction { function, .. } => {
            for arg in &function.args {
                collect_expr(arg, out);
            }
            for expr in &function.partition_by {
                collect_expr(expr, out);
            }
            for order in &function.order_by {
                collect_expr(&order.expr, out);
            }
        }
        SelectItem::Expr { expr, .. } => {
            collect_expr(expr, out);
        }
        SelectItem::Wildcard | SelectItem::Column { .. } => {}
    }
}

pub(super) fn collect_expr(expr: &Expr, out: &mut Vec<FunctionCall>) {
    if let Expr::Function(function) = expr {
        out.push(function.clone());
        for arg in &function.args {
            collect_expr(arg, out);
        }
    }
    if let Expr::Binary { left, right, .. } = expr {
        collect_expr(left, out);
        collect_expr(right, out);
    }
    if let Expr::IsNull { expr, .. } = expr {
        collect_expr(expr, out);
    }
    if let Expr::InList { expr, values, .. } = expr {
        collect_expr(expr, out);
        for value in values {
            collect_expr(value, out);
        }
    }
    if let Expr::Between {
        expr, low, high, ..
    } = expr
    {
        collect_expr(expr, out);
        collect_expr(low, out);
        collect_expr(high, out);
    }
    if let Expr::Cast { expr, .. } = expr {
        collect_expr(expr, out);
    }
    if let Expr::Not { expr } = expr {
        collect_expr(expr, out);
    }
}

pub(super) fn recursive_cte_references_self(statement: &ParsedStatement, cte_name: &str) -> bool {
    match &statement.statement {
        QueryStatement::Select(select) => source_references_cte(&select.source, cte_name),
        _ => false,
    }
}

pub(super) fn source_references_cte(source: &QuerySource, cte_name: &str) -> bool {
    match source {
        QuerySource::Cte(name) | QuerySource::Collection(name) => {
            name.eq_ignore_ascii_case(cte_name)
        }
        QuerySource::SingleRow => false,
        QuerySource::Subquery { select, .. } => source_references_cte(&select.source, cte_name),
        QuerySource::Join { left, right, .. } => {
            source_references_cte(left, cte_name) || source_references_cte(right, cte_name)
        }
    }
}
