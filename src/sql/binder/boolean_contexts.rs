use super::coalesce_results::ResultTypes;
use super::{
    BinaryOp, BindingContext, CassieError, Catalog, DataType, Expr, QuerySource, SelectItem,
    SelectStatement,
};

#[path = "boolean_aggregates.rs"]
mod boolean_aggregates;
#[path = "declared_functions.rs"]
mod declared_functions;
#[path = "row_predicates.rs"]
mod row_predicates;

pub(super) fn validate_boolean_aggregate(
    function: &mut crate::sql::ast::FunctionCall,
    types: &ResultTypes,
) -> Result<(), CassieError> {
    super::bytea_functions::qualify(function, types)?;
    if let Some(metadata) =
        crate::sql::functions::function(&function.name).filter(|metadata| metadata.is_aggregate)
    {
        for argument in &function.args {
            if matches!(argument, Expr::StringLiteral(_))
                || matches!(argument, Expr::Column(name) if name == "*")
            {
                continue;
            }
            if let Some(data_type) = types.expression_type(argument) {
                if !crate::sql::functions::aggregate_accepts_input(metadata.id, &data_type) {
                    return Err(boolean_aggregates::undefined_function(function, types));
                }
                if function.is_distinct()
                    && !crate::sql::functions::has_distinct_equality(&data_type)
                {
                    return Err(CassieError::Execution(format!(
                        "could not identify an equality operator for type {}",
                        boolean_aggregates::type_name(&data_type)
                    )));
                }
            }
        }
    }
    if function.has_modifiers() {
        if !crate::sql::functions::is_aggregate_function(&function.name) {
            let modifier = if function.is_distinct() {
                Some("DISTINCT")
            } else if !function.order_by.is_empty() {
                Some("ORDER BY")
            } else if function.filter.is_some() {
                Some("FILTER")
            } else {
                None
            };
            if let Some(modifier) = modifier {
                return Err(CassieError::Execution(format!(
                    "{modifier} specified, but {} is not an aggregate function",
                    function.name.to_ascii_lowercase()
                )));
            }
        }
        if let Some(filter) = &mut function.filter {
            if let Expr::StringLiteral(input) = filter.as_ref() {
                let value = crate::types::boolean::parse_text(input).ok_or_else(|| {
                    CassieError::Execution(format!(
                        "invalid input syntax for type boolean: {input:?}"
                    ))
                })?;
                **filter = Expr::BoolLiteral(value);
            }
            if let Some(data_type) = types.expression_type(filter) {
                if !matches!(data_type, DataType::Boolean | DataType::Null) {
                    return Err(CassieError::Execution(format!(
                        "argument of FILTER must be type boolean, not type {}",
                        boolean_aggregates::type_name(&data_type)
                    )));
                }
            }
        }
        if function.is_distinct()
            && function.order_by.iter().any(|order| {
                !function
                    .args
                    .iter()
                    .any(|argument| argument.structurally_eq(&order.expr))
            })
        {
            return Err(CassieError::Execution(
                "in an aggregate with DISTINCT, ORDER BY expressions must appear in argument list"
                    .into(),
            ));
        }
    }
    boolean_aggregates::validate(function, types)
}

pub(super) fn qualify_declared_boolean_functions(
    select: &mut SelectStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    if !declared_functions::needs_qualification(select) {
        return Ok(());
    }
    let types = ResultTypes::for_source(&select.source, &select.ctes, catalog, context)?;
    declared_functions::qualify_declared(select, &types);
    Ok(())
}

pub(super) fn validate_select(
    select: &mut SelectStatement,
    catalog: &Catalog,
    context: &BindingContext,
    outer: &super::CteScope,
) -> Result<(), CassieError> {
    let schemas = outer
        .iter()
        .map(|(name, binding)| {
            (
                name.clone(),
                crate::types::Schema {
                    fields: binding.row_fields.clone(),
                },
            )
        })
        .collect();
    let types = ResultTypes::for_scope(&select.source, &select.ctes, catalog, context, &schemas)?
        .with_controls(outer.controls());
    validate_select_with_types(select, &types, catalog, context)
}

pub(super) fn validate_select_with_types(
    select: &mut SelectStatement,
    types: &ResultTypes,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    declared_functions::qualify_declared(select, types);
    if let Some(filter) = &mut select.filter {
        require_boolean(filter, types, "WHERE")?;
    }
    if let Some(having) = &mut select.having {
        require_boolean(having, types, "HAVING")?;
    }
    validate_source_predicates(&mut select.source, types, catalog, context)?;
    for item in &mut select.projection {
        super::function_casts::lower_item(item, types)?;
        match item {
            SelectItem::Expr { expr, .. } => validate_expression(expr, types, catalog, context)?,
            SelectItem::Function { function, .. } => {
                for argument in function.expressions_mut() {
                    validate_expression(argument, types, catalog, context)?;
                }
                super::coalesce_coercion::coerce_function(function, types);
                validate_boolean_aggregate(function, types)?;
            }
            SelectItem::WindowFunction { function, .. } => {
                for expression in function
                    .args
                    .iter_mut()
                    .chain(&mut function.partition_by)
                    .chain(function.order_by.iter_mut().map(|order| &mut order.expr))
                {
                    validate_expression(expression, types, catalog, context)?;
                }
            }
            SelectItem::Wildcard | SelectItem::Column { .. } => {}
        }
        super::conditional_types::validate_item(item, types)?;
    }
    for expression in select
        .filter
        .iter_mut()
        .chain(&mut select.distinct_on)
        .chain(&mut select.group_by)
        .chain(&mut select.having)
        .chain(select.order.iter_mut().map(|order| &mut order.expr))
    {
        validate_expression(expression, types, catalog, context)?;
        super::conditional_types::validate_expression(expression, types)?;
    }
    for cte in &mut select.ctes {
        match &mut cte.query {
            super::CteQuery::Simple(statement) => {
                validate_nested_statement(statement, types, false, catalog, context)?;
            }
            super::CteQuery::Recursive {
                base, recursive, ..
            } => {
                validate_nested_statement(base, types, false, catalog, context)?;
                validate_nested_statement(recursive, types, false, catalog, context)?;
            }
        }
    }
    if let Some(set) = &mut select.set {
        let nested = ResultTypes::for_scope_with_parent(
            &set.right.source,
            &set.right.ctes,
            catalog,
            context,
            types,
            types.parameter_types(),
        )?
        .with_resolved_coalesce_domains(types.resolved_coalesce_domains());
        validate_select_with_types(&mut set.right, &nested, catalog, context)?;
    }
    Ok(())
}

fn validate_source_predicates(
    source: &mut QuerySource,
    types: &ResultTypes,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    match source {
        QuerySource::Values { rows } => {
            super::values::coerce_with_controls(
                rows,
                &std::collections::HashMap::new(),
                types.parameter_types(),
                types.controls(),
            )?;
            for expr in rows.iter_mut().flatten() {
                validate_expression(expr, types, catalog, context)?;
            }
        }

        QuerySource::Aliased { source, .. } => {
            validate_source_predicates(source, types, catalog, context)?;
        }
        QuerySource::Join {
            left, right, on, ..
        } => {
            require_boolean(on, types, "JOIN ON")?;
            validate_expression(on, types, catalog, context)?;
            super::conditional_types::validate_expression(on, types)?;
            validate_source_predicates(left, types, catalog, context)?;
            validate_source_predicates(right, types, catalog, context)?;
        }
        QuerySource::Subquery {
            select, lateral, ..
        } => {
            let inner = ResultTypes::for_scope_with_parent(
                &select.source,
                &select.ctes,
                catalog,
                context,
                types,
                types.parameter_types(),
            )?
            .with_resolved_coalesce_domains(types.resolved_coalesce_domains());
            let inner = if *lateral {
                inner.with_outer_fields(types)
            } else {
                inner
            };
            validate_select_with_types(select, &inner, catalog, context)?;
        }
        QuerySource::Collection(_)
        | QuerySource::Cte(_)
        | QuerySource::SingleRow
        | QuerySource::TableFunction { .. } => {}
    }
    Ok(())
}

fn require_boolean(expr: &mut Expr, types: &ResultTypes, context: &str) -> Result<(), CassieError> {
    if let Expr::StringLiteral(text) = expr {
        match crate::types::boolean::parse_text(text) {
            Some(value) => *expr = Expr::BoolLiteral(value),
            None => {
                return Err(CassieError::Planner(format!(
                    "invalid input syntax for type boolean: {text:?}"
                )));
            }
        }
    }
    match types.expression_type(expr) {
        Some(DataType::Boolean | DataType::Null) | None => Ok(()),
        Some(data_type) => Err(CassieError::Planner(format!(
            "{context} expression must have BOOLEAN type (found {data_type:?})"
        ))),
    }
}

pub(super) fn validate_predicate(
    expr: &mut Expr,
    types: &ResultTypes,
    label: &str,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    require_boolean(expr, types, label)?;
    validate_expression(expr, types, catalog, context)?;
    super::conditional_types::validate_expression(expr, types)?;
    Ok(())
}

pub(super) fn validate_value(
    expr: &mut Expr,
    types: &ResultTypes,
    expected: Option<&DataType>,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    if expected == Some(&DataType::Boolean) {
        require_boolean(expr, types, "Boolean assignment")?;
    }
    validate_expression(expr, types, catalog, context)?;
    super::conditional_types::validate_expression(expr, types)?;
    Ok(())
}

fn validate_expression(
    expr: &mut Expr,
    types: &ResultTypes,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    super::function_casts::lower_expression(expr, types)?;
    match expr {
        Expr::RowCompare(comparison) => {
            for field in comparison.left.iter_mut().chain(&mut comparison.right) {
                validate_expression(field, types, catalog, context)?;
            }
            row_predicates::qualify(comparison, types)?;
        }
        Expr::Binary {
            left,
            op: BinaryOp::And | BinaryOp::Or,
            right,
        } => {
            require_boolean(left, types, "Boolean operator")?;
            require_boolean(right, types, "Boolean operator")?;
            validate_expression(left, types, catalog, context)?;
            validate_expression(right, types, catalog, context)?;
        }
        Expr::Binary { left, op, right } => {
            if matches!(
                op,
                BinaryOp::Eq
                    | BinaryOp::NotEq
                    | BinaryOp::IsDistinctFrom
                    | BinaryOp::IsNotDistinctFrom
                    | BinaryOp::Lt
                    | BinaryOp::Lte
                    | BinaryOp::Gt
                    | BinaryOp::Gte
            ) {
                canonicalize_boolean_pair(left, right, types)?;
                coerce_numeric_pair(left, right, types, catalog);
                validate_concatenation_comparison(left, right, types)?;
            }
            validate_expression(left, types, catalog, context)?;
            validate_expression(right, types, catalog, context)?;
            resolve_integer_operator(left, op, right, types)?;
            super::character_comparison::qualify(op, left, right, types);
        }
        prefix @ (Expr::Not { .. } | Expr::UnaryNumeric { .. }) => {
            validate_prefix(prefix, types, catalog, context)?;
        }
        Expr::Case {
            operand,
            branches,
            else_expr,
        } => {
            validate_case(operand, branches, else_expr, types, catalog, context)?;
        }
        expression @ Expr::Cast { .. } => validate_cast(expression, types, catalog, context)?,
        Expr::TruthTest {
            expr,
            truth,
            negated,
        } => {
            validate_truth_operand(expr, *truth, *negated, types)?;
            validate_expression(expr, types, catalog, context)?;
        }
        Expr::IntegerNegate { expr, .. } | Expr::IsNull { expr, .. } => {
            validate_expression(expr, types, catalog, context)?;
        }
        Expr::InList { expr, values, .. } => {
            validate_expression(expr, types, catalog, context)?;
            for value in values {
                canonicalize_boolean_pair(expr, value, types)?;
                validate_expression(value, types, catalog, context)?;
            }
        }
        expression @ Expr::Like { .. } => {
            validate_like_expression(expression, types, catalog, context)?;
        }
        expression @ Expr::Between { .. } => {
            validate_between_expression(expression, types, catalog, context)?;
        }
        Expr::Function(function) => {
            for argument in function.expressions_mut() {
                validate_expression(argument, types, catalog, context)?;
            }
            super::coalesce_coercion::coerce_function(function, types);
            validate_boolean_aggregate(function, types)?;
        }
        Expr::Exists(statement) => {
            validate_nested_statement(statement, types, true, catalog, context)?;
        }
        Expr::NumericLiteral(_) => {
            return Err(CassieError::Unsupported(
                "unmaterialized NUMERIC literal requires an explicit primitive cast".into(),
            ))
        }
        Expr::Column(_)
        | Expr::Param(_)
        | Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::IntegerLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Null => {}
    }
    Ok(())
}

fn validate_between_expression(
    expression: &mut Expr,
    types: &ResultTypes,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    let Expr::Between {
        expr,
        low,
        high,
        symmetric,
        ..
    } = expression
    else {
        unreachable!("BETWEEN validation dispatch")
    };
    super::symmetric_between::validate([expr, low, high], *symmetric, types)?;
    canonicalize_boolean_pair(expr, low, types)?;
    canonicalize_boolean_pair(expr, high, types)?;
    canonicalize_boolean_pair(low, high, types)?;
    validate_expression(expr, types, catalog, context)?;
    validate_expression(low, types, catalog, context)?;
    validate_expression(high, types, catalog, context)?;
    Ok(())
}

fn validate_concatenation_comparison(
    left: &mut Expr,
    right: &mut Expr,
    types: &ResultTypes,
) -> Result<(), CassieError> {
    if types.parameter_types().is_empty()
        || (!contains_concatenation(left) && !contains_concatenation(right))
    {
        return Ok(());
    }
    // An uncast SQL string is unknown until its comparison signature is known.
    let literal = if types.expression_type(left) == Some(DataType::Bytea)
        && matches!(right, Expr::StringLiteral(_))
    {
        Some(&mut *right)
    } else if types.expression_type(right) == Some(DataType::Bytea)
        && matches!(left, Expr::StringLiteral(_))
    {
        Some(&mut *left)
    } else {
        None
    };
    if let Some(literal) = literal {
        *literal = Expr::Cast {
            expr: Box::new(std::mem::replace(literal, Expr::Null)),
            data_type: DataType::Bytea,
            source_type: None,
            identity: crate::sql::ast::CastIdentity::Scalar,
        };
    }
    let left = types.expression_type(left);
    let right = types.expression_type(right);
    if let (Some(left), Some(right)) = (&left, &right) {
        if !matches!(left, DataType::Null)
            && !matches!(right, DataType::Null)
            && (left == &DataType::Bytea || right == &DataType::Bytea)
            && left != right
        {
            return Err(CassieError::Planner(format!(
                "incompatible comparison operands: {left:?} and {right:?}"
            )));
        }
    }
    let family = |kind: Option<&DataType>| {
        kind.filter(|kind| **kind != DataType::Null)
            .map(super::validation::data_type_family)
    };
    super::validation::require_compatible_families(
        family(left.as_ref()),
        family(right.as_ref()),
        "comparison",
    )
}

fn contains_concatenation(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::Binary {
            op: BinaryOp::Concat | BinaryOp::ByteaConcat,
            ..
        }
    ) || expr
        .try_visit_children(|child| {
            if contains_concatenation(child) {
                Err(())
            } else {
                Ok(())
            }
        })
        .is_err()
}

fn resolve_integer_operator(
    left: &mut Expr,
    op: &mut BinaryOp,
    right: &mut Expr,
    types: &ResultTypes,
) -> Result<(), CassieError> {
    super::string_operators::qualify(
        left,
        op,
        right,
        types.expression_type(left).as_ref(),
        types.expression_type(right).as_ref(),
    );
    // Initial binding has no parameter OIDs. Select every binary signature
    // in the concrete pass, before the executor erases those input identities.
    if !types.parameter_types().is_empty()
        || (!super::validation::expr_contains_parameters(left)
            && !super::validation::expr_contains_parameters(right))
    {
        super::integer_operators::resolve(
            op,
            types.expression_type(left).as_ref(),
            types.expression_type(right).as_ref(),
        )?;
    }
    Ok(())
}

fn validate_prefix(
    prefix: &mut Expr,
    types: &ResultTypes,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    match prefix {
        Expr::Not { expr } => {
            require_boolean(expr, types, "NOT")?;
            validate_expression(expr, types, catalog, context)
        }
        Expr::UnaryNumeric { expr, .. } => {
            validate_expression(expr, types, catalog, context)?;
            let data_type = types.expression_type(expr);
            super::unary_numeric::resolve(prefix, data_type.as_ref())
        }
        _ => unreachable!("prefix matched by expression validation"),
    }
}

fn validate_cast(
    expression: &mut Expr,
    types: &ResultTypes,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    let Expr::Cast {
        expr,
        data_type,
        source_type,
        identity,
    } = expression
    else {
        unreachable!("only casts reach cast validation");
    };
    crate::sql::literal_inputs::validate_with_controls(
        expr,
        data_type,
        *identity,
        types.controls(),
    )?;
    if matches!(expr.as_ref(), Expr::NumericLiteral(_)) {
        validate_numeric_literal_cast(data_type, *identity)?;
    } else {
        validate_expression(expr, types, catalog, context)?;
    }
    *source_type = cast_source_type(expr, data_type, types, catalog);
    Ok(())
}

pub(super) fn validate_numeric_literal_cast(
    data_type: &DataType,
    identity: crate::sql::ast::CastIdentity,
) -> Result<(), CassieError> {
    if identity != crate::sql::ast::CastIdentity::Scalar
        || !matches!(
            data_type,
            DataType::SmallInt
                | DataType::Int
                | DataType::BigInt
                | DataType::Float
                | DataType::Text
                | DataType::Varchar { .. }
        )
    {
        return Err(CassieError::Unsupported(
            "exact NUMERIC literal requires a supported primitive cast".into(),
        ));
    }
    Ok(())
}

fn validate_case(
    operand: &mut Option<Box<Expr>>,
    branches: &mut [(Expr, Expr)],
    else_expr: &mut Option<Box<Expr>>,
    types: &ResultTypes,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    let searched = operand.is_none();
    if let Some(operand) = operand {
        validate_expression(operand, types, catalog, context)?;
    }
    for (condition, result) in branches {
        if searched {
            require_boolean(condition, types, "CASE WHEN")?;
        } else if let Some(operand) = operand {
            canonicalize_boolean_pair(operand, condition, types)?;
        }
        validate_expression(condition, types, catalog, context)?;
        validate_expression(result, types, catalog, context)?;
    }
    if let Some(result) = else_expr {
        validate_expression(result, types, catalog, context)?;
    }
    Ok(())
}

fn cast_source_type(
    expr: &Expr,
    target: &DataType,
    types: &ResultTypes,
    catalog: &Catalog,
) -> Option<DataType> {
    if *target == DataType::Date {
        return types
            .expression_type(expr)
            .filter(|source| *source == DataType::Timestamp);
    }
    if *target == DataType::Bytea || crate::types::IntegerWidth::from_data_type(target).is_some() {
        return types.expression_type(expr).filter(|source| {
            (*target == DataType::Bytea
                && crate::types::IntegerWidth::from_data_type(source).is_some())
                || (*source == DataType::Bytea
                    && crate::types::IntegerWidth::from_data_type(target).is_some())
        });
    }
    if !matches!(
        target,
        DataType::Text | DataType::Varchar { .. } | DataType::Char { length: None }
    ) {
        return None;
    }
    // These selected output conversions need source identity. Unresolved
    // NUMERIC carriers must not be relabeled as proven FLOAT8, and no heap
    // type descriptor is cloned just to annotate an unrelated cast.
    types.expression_type(expr).filter(|kind| {
        matches!(
            kind,
            DataType::Time | DataType::Timestamp | DataType::Char { .. } | DataType::ObjectId(_)
        ) || (*kind == DataType::Float && types.proven_float8(expr, catalog))
    })
}

fn coerce_numeric_pair(left: &mut Expr, right: &mut Expr, types: &ResultTypes, catalog: &Catalog) {
    use crate::sql::numeric_comparison::NumericComparisonDomain;

    let (Some(left_type), Some(right_type)) =
        (types.expression_type(left), types.expression_type(right))
    else {
        return;
    };
    // Carrier metadata alone cannot establish a PostgreSQL float8 signature.
    if (left_type == DataType::Float && !types.proven_float8(left, catalog))
        || (right_type == DataType::Float && !types.proven_float8(right, catalog))
    {
        return;
    }
    if NumericComparisonDomain::resolve(&left_type, &right_type) == NumericComparisonDomain::Float8
    {
        *left = super::result_coercion::float(std::mem::replace(left, Expr::Null));
        *right = super::result_coercion::float(std::mem::replace(right, Expr::Null));
    }
}

fn canonicalize_boolean_pair(
    left: &mut Expr,
    right: &mut Expr,
    types: &ResultTypes,
) -> Result<(), CassieError> {
    if types.expression_type(left) == Some(DataType::Boolean) {
        require_boolean(right, types, "Boolean comparison")?;
    }
    if types.expression_type(right) == Some(DataType::Boolean) {
        require_boolean(left, types, "Boolean comparison")?;
    }
    Ok(())
}

fn validate_nested_statement(
    statement: &mut super::ParsedStatement,
    outer: &ResultTypes,
    correlated: bool,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    if let super::QueryStatement::Select(select) = &mut statement.statement {
        let names = outer.cte_schemas().keys().cloned().collect();
        let mut scoped = select.clone();
        resolve_nested_sources(&mut scoped, &names, catalog, context)?;
        let mut types = ResultTypes::for_scope_with_parent(
            &scoped.source,
            &scoped.ctes,
            catalog,
            context,
            outer,
            outer.parameter_types(),
        )?
        .with_resolved_coalesce_domains(outer.resolved_coalesce_domains());
        if correlated {
            types = types.with_outer_fields(outer);
        }
        validate_select_with_types(select, &types, catalog, context)?;
    }
    Ok(())
}

pub(super) fn resolve_nested_sources(
    select: &mut SelectStatement,
    outer: &super::HashSet<String>,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    let mut names = outer.clone();
    for cte in &mut select.ctes {
        if matches!(cte.query, super::CteQuery::Recursive { .. }) {
            names.insert(cte.name.to_ascii_lowercase());
        }
        match &mut cte.query {
            super::CteQuery::Simple(statement) => {
                resolve_nested_statement_sources(statement, &names, catalog, context)?;
            }
            super::CteQuery::Recursive {
                base, recursive, ..
            } => {
                resolve_nested_statement_sources(base, &names, catalog, context)?;
                resolve_nested_statement_sources(recursive, &names, catalog, context)?;
            }
        }
        names.insert(cte.name.to_ascii_lowercase());
    }
    resolve_nested_source(&mut select.source, &names, catalog, context)?;
    if let Some(set) = &mut select.set {
        resolve_nested_sources(&mut set.right, &names, catalog, context)?;
    }
    Ok(())
}

fn resolve_nested_statement_sources(
    statement: &mut super::ParsedStatement,
    names: &super::HashSet<String>,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    if let super::QueryStatement::Select(select) = &mut statement.statement {
        resolve_nested_sources(select, names, catalog, context)?;
    }
    Ok(())
}

fn resolve_nested_source(
    source: &mut QuerySource,
    names: &super::HashSet<String>,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    match source {
        QuerySource::Aliased { source, .. } => {
            resolve_nested_source(source, names, catalog, context)?;
        }
        QuerySource::Collection(name) => {
            if names.contains(&name.to_ascii_lowercase()) {
                *source = QuerySource::Cte(name.to_string());
            } else if !(name.components().len() == 3
                && name.components()[0]
                    .value()
                    .eq_ignore_ascii_case(&context.database)
                && catalog.relation_exists(name))
            {
                let resolved = super::resolve_relation_path(name, catalog, context)?;
                *name = crate::sql::ast::IdentifierPath::parse(&resolved)
                    .map_err(CassieError::Planner)?;
            }
        }
        QuerySource::Subquery { select, .. } => {
            resolve_nested_sources(select, names, catalog, context)?;
        }
        QuerySource::Join { left, right, .. } => {
            resolve_nested_source(left, names, catalog, context)?;
            resolve_nested_source(right, names, catalog, context)?;
        }
        QuerySource::Cte(_)
        | QuerySource::Values { .. }
        | QuerySource::SingleRow
        | QuerySource::TableFunction { .. } => {}
    }
    Ok(())
}

fn validate_truth_operand(
    expr: &mut Expr,
    truth: crate::sql::ast::TruthValue,
    negated: bool,
    types: &ResultTypes,
) -> Result<(), CassieError> {
    if let Expr::StringLiteral(text) = expr {
        let value = crate::types::boolean::parse_text(text).ok_or_else(|| {
            CassieError::Execution(format!("invalid input syntax for type boolean: {text:?}"))
        })?;
        *expr = Expr::BoolLiteral(value);
    }
    if let Some(kind) = types.expression_type(expr) {
        if !matches!(kind, DataType::Boolean | DataType::Null) {
            return Err(CassieError::Execution(format!(
                "argument of {} must be type boolean, not type {}",
                truth.label(negated),
                boolean_aggregates::type_name(&kind)
            )));
        }
    }
    Ok(())
}

fn validate_like_expression(
    expression: &mut Expr,
    types: &ResultTypes,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    let Expr::Like {
        expr,
        pattern,
        escape,
    } = expression
    else {
        unreachable!("LIKE validation dispatch")
    };
    for operand in [&mut **expr, &mut **pattern, &mut **escape] {
        validate_expression(operand, types, catalog, context)?;
    }
    super::like_predicates::validate(expr, pattern, escape, types)
}
