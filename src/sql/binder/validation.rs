use super::{
    BinaryOp, CassieError, Catalog, CommonTableExpression, CteQuery, DataType, Expr, FunctionCall,
    HashSet, OrderExpr, ParsedStatement, QuerySource, QueryStatement, SelectItem, SelectStatement,
};

#[path = "validation_case.rs"]
mod case;
use case::case_operand_family;
#[path = "validation_vector.rs"]
mod vector;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OperandFamily {
    Numeric,
    Boolean,
    Text,
    Temporal,
    Vector,
    Json,
    /// An extended family with its own operators.
    Typed(crate::types::extended::ExtendedFamily),
    Other,
}

impl OperandFamily {
    const fn label(self) -> &'static str {
        match self {
            Self::Numeric => "numeric",
            Self::Boolean => "boolean",
            Self::Text => "text",
            Self::Temporal => "temporal",
            Self::Vector => "vector",
            Self::Json => "json",
            Self::Typed(_) => "typed",
            Self::Other => "other",
        }
    }
}

pub(super) fn validate_select_operand_families(
    select: &SelectStatement,
    catalog: &Catalog,
) -> Result<(), CassieError> {
    let field_types =
        crate::sql::source_field_type_map_with_ctes(&select.source, &select.ctes, catalog);
    for item in &select.projection {
        validate_select_item_operand_families(item, &field_types)?;
    }
    if let Some(filter) = &select.filter {
        validate_expression_operand_families(filter, &field_types)?;
    }
    for expression in &select.distinct_on {
        validate_expression_operand_families(expression, &field_types)?;
    }
    for expression in &select.group_by {
        validate_expression_operand_families(expression, &field_types)?;
    }
    if let Some(having) = &select.having {
        validate_expression_operand_families(having, &field_types)?;
    }
    for order in &select.order {
        validate_expression_operand_families(&order.expr, &field_types)?;
    }
    Ok(())
}

fn validate_select_item_operand_families(
    item: &SelectItem,
    field_types: &crate::sql::FieldTypeMap,
) -> Result<(), CassieError> {
    match item {
        SelectItem::Wildcard | SelectItem::Column { .. } => Ok(()),
        SelectItem::Function { function, .. } => {
            for argument in function.expressions() {
                validate_expression_operand_families(argument, field_types)?;
            }
            vector::validate_vector_function_query(function, field_types)?;
            Ok(())
        }
        SelectItem::WindowFunction { function, .. } => {
            for argument in &function.args {
                validate_expression_operand_families(argument, field_types)?;
            }
            for expression in &function.partition_by {
                validate_expression_operand_families(expression, field_types)?;
            }
            for order in &function.order_by {
                validate_expression_operand_families(&order.expr, field_types)?;
            }
            Ok(())
        }
        SelectItem::Expr { expr, .. } => validate_expression_operand_families(expr, field_types),
    }
}

pub(super) fn validate_expression_operand_families(
    expr: &Expr,
    field_types: &crate::sql::FieldTypeMap,
) -> Result<(), CassieError> {
    let _ = expression_operand_family(expr, field_types)?;
    Ok(())
}

fn expression_operand_family(
    expr: &Expr,
    field_types: &crate::sql::FieldTypeMap,
) -> Result<Option<OperandFamily>, CassieError> {
    match expr {
        Expr::Column(name) => Ok(crate::sql::field_type_for_column(field_types, name)
            .filter(|data_type| !matches!(data_type, DataType::Null))
            .map(data_type_family)),
        Expr::StringLiteral(value) => Ok(Some(string_literal_family(value))),
        Expr::NumberLiteral(_) | Expr::IntegerLiteral(_) | Expr::NumericLiteral(_) => {
            Ok(Some(OperandFamily::Numeric))
        }
        Expr::BoolLiteral(_) => Ok(Some(OperandFamily::Boolean)),
        Expr::Null | Expr::Param(_) | Expr::Exists(_) => Ok(None),
        Expr::Subquery(subquery) => Ok(subquery
            .result_type()
            .filter(|data_type| !matches!(data_type, DataType::Null))
            .map(|data_type| data_type_family(&data_type))),
        Expr::Case {
            operand,
            branches,
            else_expr,
        } => case_operand_family(
            operand.as_deref(),
            branches,
            else_expr.as_deref(),
            field_types,
        ),
        Expr::Function(function) => {
            for argument in function.expressions() {
                validate_expression_operand_families(argument, field_types)?;
            }
            vector::validate_vector_function_query(function, field_types)?;
            Ok(temporal_function_family(function, field_types))
        }
        Expr::RowCompare(comparison) => {
            for (left, right) in comparison.left.iter().zip(&comparison.right) {
                binary_operand_family(left, &comparison.op.binary(), right, field_types)?;
            }
            Ok(Some(OperandFamily::Boolean))
        }
        Expr::UnaryNumeric { expr, .. } | Expr::IntegerNegate { expr, .. } => {
            match expression_operand_family(expr, field_types)? {
                Some(family @ (OperandFamily::Typed(_) | OperandFamily::Temporal)) => {
                    Ok(Some(family))
                }
                _ => Ok(Some(OperandFamily::Numeric)),
            }
        }
        Expr::Cast {
            expr, data_type, ..
        } => {
            validate_expression_operand_families(expr, field_types)?;
            Ok(Some(data_type_family(data_type)))
        }
        Expr::IsNull { expr, .. } | Expr::TruthTest { expr, .. } | Expr::Not { expr } => {
            validate_expression_operand_families(expr, field_types)?;
            Ok(Some(OperandFamily::Boolean))
        }
        Expr::Binary { left, op, right } => binary_operand_family(left, op, right, field_types),
        Expr::InList { expr, values, .. } => {
            let expression_family = expression_operand_family(expr, field_types)?;
            for value in values {
                let value_family = expression_operand_family(value, field_types)?;
                if validate_typed_string_comparison(expr, value, field_types)? {
                    continue;
                }
                let (expression_family, value_family) = contextual_string_literal_families(
                    expr,
                    value,
                    expression_family,
                    value_family,
                    true,
                )?;
                require_compatible_families(expression_family, value_family, "IN")?;
            }
            Ok(Some(OperandFamily::Boolean))
        }
        Expr::Like {
            expr,
            pattern,
            escape,
            ..
        } => {
            for operand in [expr, pattern, escape] {
                expression_operand_family(operand, field_types)?;
            }
            Ok(Some(OperandFamily::Boolean))
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            let expression_family = expression_operand_family(expr, field_types)?;
            let low_family = expression_operand_family(low, field_types)?;
            let high_family = expression_operand_family(high, field_types)?;
            if !validate_typed_string_comparison(expr, low, field_types)? {
                require_compatible_families(expression_family, low_family, "BETWEEN")?;
            }
            if !validate_typed_string_comparison(expr, high, field_types)? {
                require_compatible_families(expression_family, high_family, "BETWEEN")?;
            }
            Ok(Some(OperandFamily::Boolean))
        }
    }
}

/// The family of a date/time builtin's result, when its type is known.
fn temporal_function_family(
    function: &crate::sql::ast::FunctionCall,
    field_types: &crate::sql::FieldTypeMap,
) -> Option<OperandFamily> {
    let argument_type = |index: usize| {
        function
            .args
            .get(index)
            .and_then(|argument| super::known_expr_type(argument, field_types))
    };
    crate::sql::functions::temporal_return_type(
        &function.name.to_ascii_lowercase(),
        &function.args,
        &argument_type,
    )
    .map(|data_type| data_type_family(&data_type))
}

fn binary_operand_family(
    left: &Expr,
    op: &BinaryOp,
    right: &Expr,
    field_types: &crate::sql::FieldTypeMap,
) -> Result<Option<OperandFamily>, CassieError> {
    let left_family = expression_operand_family(left, field_types)?;
    let right_family = expression_operand_family(right, field_types)?;
    if matches!(op, BinaryOp::JsonbDeletePath) {
        return json_delete_path_family(left, right, left_family, right_family, field_types);
    }
    if selects_typed_operator(op, left_family, right_family) {
        return Ok(None);
    }
    if matches!(op, BinaryOp::Sub) && left_family == Some(OperandFamily::Json) {
        return json_delete_key_family(right, right_family, field_types);
    }
    match op {
        BinaryOp::JsonbKeyExists | BinaryOp::JsonbKeyExistsAny | BinaryOp::JsonbKeyExistsAll => {
            json_key_operator_family(left, op, right, (left_family, right_family), field_types)
        }
        BinaryOp::JsonbDeletePath => Ok(Some(OperandFamily::Json)),
        BinaryOp::ByteaConcat => Ok(Some(OperandFamily::Other)),
        BinaryOp::Concat => concat_operand_family(left, right, left_family, right_family),
        BinaryOp::And | BinaryOp::Or | BinaryOp::Typed { .. } | BinaryOp::Quantified { .. } => {
            Ok(Some(OperandFamily::Boolean))
        }
        BinaryOp::Eq
        | BinaryOp::NotEq
        | BinaryOp::IsDistinctFrom
        | BinaryOp::IsNotDistinctFrom
        | BinaryOp::Lt
        | BinaryOp::Lte
        | BinaryOp::Gt
        | BinaryOp::Gte => {
            if validate_typed_string_comparison(left, right, field_types)? {
                return Ok(Some(OperandFamily::Boolean));
            }
            let (left_family, right_family) = contextual_string_literal_families(
                left,
                right,
                left_family,
                right_family,
                matches!(
                    op,
                    BinaryOp::Eq
                        | BinaryOp::NotEq
                        | BinaryOp::IsDistinctFrom
                        | BinaryOp::IsNotDistinctFrom
                ),
            )?;
            require_compatible_families(left_family, right_family, "comparison")?;
            Ok(Some(OperandFamily::Boolean))
        }
        BinaryOp::Like => {
            require_family(left_family, OperandFamily::Text, "LIKE")?;
            require_family(right_family, OperandFamily::Text, "LIKE")?;
            Ok(Some(OperandFamily::Boolean))
        }
        BinaryOp::Power
        | BinaryOp::Integer { .. }
        | BinaryOp::Add
        | BinaryOp::Sub
        | BinaryOp::Mul
        | BinaryOp::Div
        | BinaryOp::Modulo
        | BinaryOp::Bitwise { .. } => {
            // Date/time arithmetic types are resolved by the temporal operators.
            if [left_family, right_family].contains(&Some(OperandFamily::Temporal)) {
                return Ok(None);
            }
            require_family(left_family, OperandFamily::Numeric, "arithmetic")?;
            require_family(right_family, OperandFamily::Numeric, "arithmetic")?;
            Ok(Some(OperandFamily::Numeric))
        }
        BinaryOp::PgvectorCosine | BinaryOp::PgvectorL2 | BinaryOp::PgvectorDot => {
            require_family(left_family, OperandFamily::Vector, "vector distance")?;
            require_family(right_family, OperandFamily::Vector, "vector distance")?;
            vector::validate_vector_query_literals(left, right, field_types)?;
            Ok(Some(OperandFamily::Numeric))
        }
    }
}

fn concat_operand_family(
    left: &Expr,
    right: &Expr,
    left_family: Option<OperandFamily>,
    right_family: Option<OperandFamily>,
) -> Result<Option<OperandFamily>, CassieError> {
    // Parameter OIDs choose the text or BYTEA overload in the typed pass.
    if expr_contains_parameters(left) || expr_contains_parameters(right) {
        return Ok(None);
    }
    if left_family == Some(OperandFamily::Json) || right_family == Some(OperandFamily::Json) {
        let json_or_null_or_literal = |expr: &Expr, family: Option<OperandFamily>| {
            family == Some(OperandFamily::Json)
                || family.is_none()
                || matches!(expr, Expr::StringLiteral(_))
        };
        if json_or_null_or_literal(left, left_family)
            && json_or_null_or_literal(right, right_family)
        {
            return Ok(Some(OperandFamily::Json));
        }
        return Err(CassieError::Execution(
            "operator does not exist: jsonb || incompatible operands".into(),
        ));
    }
    let family = |expr: &Expr, inferred| {
        if matches!(expr, Expr::StringLiteral(_)) {
            Some(OperandFamily::Text)
        } else {
            inferred
        }
    };
    let left = family(left, left_family);
    let right = family(right, right_family);
    if [left, right].into_iter().flatten().any(|family| {
        !matches!(
            family,
            OperandFamily::Numeric | OperandFamily::Boolean | OperandFamily::Text
        )
    }) {
        return Err(CassieError::Unsupported(
            "concatenation for this input layout remains pending under #793/#796".into(),
        ));
    }
    if left.is_some_and(|family| family != OperandFamily::Text)
        && right.is_some_and(|family| family != OperandFamily::Text)
    {
        return Err(CassieError::Execution(
            "operator does not exist: non-text || non-text".into(),
        ));
    }
    Ok(Some(OperandFamily::Text))
}

fn json_delete_key_family(
    right: &Expr,
    right_family: Option<OperandFamily>,
    field_types: &crate::sql::FieldTypeMap,
) -> Result<Option<OperandFamily>, CassieError> {
    validate_json_delete_key_operand(right, right_family, field_types)?;
    Ok(Some(OperandFamily::Json))
}

fn validate_json_delete_key_operand(
    right: &Expr,
    right_family: Option<OperandFamily>,
    field_types: &crate::sql::FieldTypeMap,
) -> Result<(), CassieError> {
    if matches!(right, Expr::StringLiteral(_) | Expr::Null | Expr::Param(_))
        || right_family == Some(OperandFamily::Text)
    {
        return Ok(());
    }
    let right_type = super::known_expr_type(right, field_types);
    let is_integer = right_type
        .as_ref()
        .is_some_and(|data_type| matches!(data_type, DataType::SmallInt | DataType::Int));
    let is_text_array = right_type.as_ref().is_some_and(
        |data_type| matches!(data_type, DataType::Array(element) if **element == DataType::Text),
    );
    if is_integer || is_text_array {
        return Ok(());
    }
    let right_type = right_type.map_or_else(
        || right_family.map_or_else(|| "unknown".to_owned(), |family| family.label().to_owned()),
        |data_type| data_type.type_name(),
    );
    Err(CassieError::Execution(format!(
        "operator does not exist: json - {right_type}"
    )))
}

fn json_delete_path_family(
    left: &Expr,
    right: &Expr,
    left_family: Option<OperandFamily>,
    right_family: Option<OperandFamily>,
    field_types: &crate::sql::FieldTypeMap,
) -> Result<Option<OperandFamily>, CassieError> {
    if left_family.is_some_and(|family| family != OperandFamily::Json) {
        return Err(CassieError::Execution(
            "operator does not exist: non-json #- text[]".into(),
        ));
    }
    let right_type = super::known_expr_type(right, field_types);
    if matches!(right, Expr::Null | Expr::Param(_))
        || right_type.as_ref().is_some_and(|data_type| {
            matches!(data_type, DataType::Array(element) if **element == DataType::Text)
        })
    {
        return Ok(Some(OperandFamily::Json));
    }
    let left_type = super::known_expr_type(left, field_types)
        .map_or_else(|| "unknown".to_owned(), |data_type| data_type.type_name());
    let right_type = right_type.map_or_else(
        || right_family.map_or_else(|| "unknown".to_owned(), |family| family.label().to_owned()),
        |data_type| data_type.type_name(),
    );
    Err(CassieError::Execution(format!(
        "operator does not exist: {left_type} #- {right_type}"
    )))
}

fn json_key_operator_family(
    left: &Expr,
    op: &BinaryOp,
    right: &Expr,
    families: (Option<OperandFamily>, Option<OperandFamily>),
    field_types: &crate::sql::FieldTypeMap,
) -> Result<Option<OperandFamily>, CassieError> {
    validate_json_key_operator_operands(left, op, right, families.0, families.1, field_types)?;
    Ok(Some(OperandFamily::Boolean))
}

fn validate_json_key_operator_operands(
    left: &Expr,
    op: &BinaryOp,
    right: &Expr,
    left_family: Option<OperandFamily>,
    right_family: Option<OperandFamily>,
    field_types: &crate::sql::FieldTypeMap,
) -> Result<(), CassieError> {
    let operator = match op {
        BinaryOp::JsonbKeyExists => "?",
        BinaryOp::JsonbKeyExistsAny => "?|",
        BinaryOp::JsonbKeyExistsAll => "?&",
        _ => return Ok(()),
    };
    if left_family.is_some_and(|family| family != OperandFamily::Json) {
        return Err(json_key_operator_error(
            left,
            operator,
            right,
            left_family,
            right_family,
            field_types,
        ));
    }
    if matches!(op, BinaryOp::JsonbKeyExists) {
        if !matches!(right, Expr::StringLiteral(_) | Expr::Null | Expr::Param(_))
            && right_family.is_some_and(|family| family != OperandFamily::Text)
        {
            return Err(json_key_operator_error(
                left,
                operator,
                right,
                left_family,
                right_family,
                field_types,
            ));
        }
        return Ok(());
    }
    let right_type = super::known_expr_type(right, field_types);
    match (right, right_type) {
        (Expr::StringLiteral(_) | Expr::Null | Expr::Param(_), _) => Ok(()),
        (_, Some(DataType::Array(element))) if *element == DataType::Text => Ok(()),
        (_, Some(DataType::Null) | None) if right_family.is_none() => Ok(()),
        _ => Err(json_key_operator_error(
            left,
            operator,
            right,
            left_family,
            right_family,
            field_types,
        )),
    }
}

fn json_key_operator_error(
    left: &Expr,
    operator: &str,
    right: &Expr,
    left_family: Option<OperandFamily>,
    right_family: Option<OperandFamily>,
    field_types: &crate::sql::FieldTypeMap,
) -> CassieError {
    let type_name = |expr: &Expr, family: Option<OperandFamily>| {
        super::known_expr_type(expr, field_types).map_or_else(
            || family.map_or_else(|| "unknown".to_owned(), |family| family.label().to_owned()),
            |data_type| data_type.type_name(),
        )
    };
    CassieError::Execution(format!(
        "operator does not exist: {} {operator} {}",
        type_name(left, left_family),
        type_name(right, right_family)
    ))
}

fn validate_typed_string_comparison(
    left: &Expr,
    right: &Expr,
    field_types: &crate::sql::FieldTypeMap,
) -> Result<bool, CassieError> {
    let (
        (Expr::Column(column), Expr::StringLiteral(literal))
        | (Expr::StringLiteral(literal), Expr::Column(column)),
    ) = ((left, right),)
    else {
        return Ok(false);
    };
    match crate::sql::field_type_for_column(field_types, column) {
        Some(DataType::Uuid) => {
            uuid::Uuid::parse_str(literal)
                .map_err(|_| CassieError::Planner(format!("invalid UUID literal '{literal}'")))?;
            Ok(true)
        }
        Some(DataType::Bytea) => {
            let digits = literal.strip_prefix("\\x").ok_or_else(|| {
                CassieError::Planner(format!("invalid BYTEA literal '{literal}'"))
            })?;
            if digits.len() % 2 != 0 || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(CassieError::Planner(format!(
                    "invalid BYTEA literal '{literal}'"
                )));
            }
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn contextual_string_literal_families(
    left: &Expr,
    right: &Expr,
    left_family: Option<OperandFamily>,
    right_family: Option<OperandFamily>,
    allow_json: bool,
) -> Result<(Option<OperandFamily>, Option<OperandFamily>), CassieError> {
    match (left, right, left_family, right_family) {
        (Expr::StringLiteral(literal), _, _, Some(family))
        | (_, Expr::StringLiteral(literal), Some(family), _)
            if family == OperandFamily::Text || (allow_json && family == OperandFamily::Json) =>
        {
            if family == OperandFamily::Json {
                serde_json::from_str::<serde_json::Value>(literal).map_err(|_| {
                    CassieError::Planner(format!("invalid JSON literal '{literal}'"))
                })?;
            }
            Ok((Some(family), Some(family)))
        }
        _ => Ok((left_family, right_family)),
    }
}

pub(super) fn require_compatible_families(
    left: Option<OperandFamily>,
    right: Option<OperandFamily>,
    operation: &str,
) -> Result<(), CassieError> {
    let (Some(left), Some(right)) = (left, right) else {
        return Ok(());
    };
    if families_compatible(left, right) {
        return Ok(());
    }
    Err(CassieError::Planner(format!(
        "incompatible {operation} operands: {} and {}",
        left.label(),
        right.label()
    )))
}

fn string_literal_family(value: &str) -> OperandFamily {
    if serde_json::from_str::<Vec<f32>>(value).is_ok_and(|values| !values.is_empty()) {
        OperandFamily::Vector
    } else {
        OperandFamily::Text
    }
}

fn require_family(
    family: Option<OperandFamily>,
    expected: OperandFamily,
    operation: &str,
) -> Result<(), CassieError> {
    let Some(family) = family else {
        return Ok(());
    };
    if family == expected {
        return Ok(());
    }
    Err(CassieError::Planner(format!(
        "incompatible {operation} operand family: expected {}, got {}",
        expected.label(),
        family.label()
    )))
}

/// Operators over extended families are selected by operand types at
/// binding and evaluation; comparisons keep the shared family checks.
fn selects_typed_operator(
    op: &BinaryOp,
    left: Option<OperandFamily>,
    right: Option<OperandFamily>,
) -> bool {
    let typed = |family: Option<OperandFamily>| matches!(family, Some(OperandFamily::Typed(_)));
    if !typed(left) && !typed(right) {
        return false;
    }
    !matches!(
        op,
        BinaryOp::Eq
            | BinaryOp::NotEq
            | BinaryOp::IsDistinctFrom
            | BinaryOp::IsNotDistinctFrom
            | BinaryOp::Lt
            | BinaryOp::Lte
            | BinaryOp::Gt
            | BinaryOp::Gte
            | BinaryOp::And
            | BinaryOp::Or
    )
}

fn families_compatible(left: OperandFamily, right: OperandFamily) -> bool {
    left == right
        || matches!(
            (left, right),
            (OperandFamily::Temporal, OperandFamily::Text)
                | (OperandFamily::Text, OperandFamily::Temporal)
        )
}

pub(super) const fn data_type_family(data_type: &DataType) -> OperandFamily {
    match data_type {
        DataType::SmallInt
        | DataType::Int
        | DataType::BigInt
        | DataType::Float
        | DataType::ObjectId(_) => OperandFamily::Numeric,
        DataType::Extended(kind) => match kind.family() {
            crate::types::extended::ExtendedFamily::Numeric => OperandFamily::Numeric,
            crate::types::extended::ExtendedFamily::Temporal
            | crate::types::extended::ExtendedFamily::Timespan => OperandFamily::Temporal,
            family => OperandFamily::Typed(family),
        },
        DataType::Boolean => OperandFamily::Boolean,
        DataType::Text | DataType::Char { .. } | DataType::Varchar { .. } => OperandFamily::Text,
        DataType::Date | DataType::Time | DataType::Timestamp => OperandFamily::Temporal,
        DataType::Vector(_) => OperandFamily::Vector,
        DataType::Json => OperandFamily::Json,
        DataType::Uuid | DataType::Bytea | DataType::Array(_) | DataType::Null => {
            OperandFamily::Other
        }
    }
}

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
        || select.limit.as_ref().is_some_and(expr_contains_parameters)
        || select.offset.as_ref().is_some_and(expr_contains_parameters)
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
        CteQuery::Recursive {
            base, recursive, ..
        } => {
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
        | QueryStatement::Copy(_)
        | QueryStatement::Insert(_)
        | QueryStatement::Update(_)
        | QueryStatement::Delete(_)
        | QueryStatement::Truncate(_)
        | QueryStatement::Merge(_)
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
        | QueryStatement::DropView(_)
        | QueryStatement::CreateRole(_)
        | QueryStatement::AlterRole(_)
        | QueryStatement::DropRole(_)
        | QueryStatement::GrantDatabaseConnect(_)
        | QueryStatement::RevokeDatabaseConnect(_)
        | QueryStatement::CreateIndex(_)
        | QueryStatement::DropIndex(_)
        | QueryStatement::AlterIndex(_)
        | QueryStatement::Reindex(_)
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
        | QueryStatement::EnforceRetentionPolicy(_) => false,
    }
}

pub(super) fn select_item_contains_parameters(item: &SelectItem) -> bool {
    match item {
        SelectItem::Wildcard | SelectItem::Column { .. } => false,
        SelectItem::Function { function, .. } => {
            function.expressions().any(expr_contains_parameters)
        }
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
        QuerySource::Values { rows } => rows.iter().flatten().any(expr_contains_parameters),

        QuerySource::Aliased { source, .. } => source_contains_parameters(source),
        QuerySource::Collection(_) | QuerySource::Cte(_) | QuerySource::SingleRow => false,
        QuerySource::TableFunction { function, .. } => {
            function.expressions().any(expr_contains_parameters)
        }
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
        Expr::Exists(statement) => parsed_statement_contains_parameters(statement),
        Expr::Subquery(subquery) if parsed_statement_contains_parameters(&subquery.query) => true,
        _ => expr.any_child(expr_contains_parameters),
    }
}

pub(super) fn qualified_fields(
    qualifier: &str,
    fields: impl IntoIterator<Item = String>,
) -> HashSet<String> {
    let qualifiers = crate::catalog::qualifier_variants(qualifier);
    let mut out = HashSet::new();
    for field in fields {
        let field = crate::sql::ColumnIdentifierPath::from_field_name(&field).lookup_key();
        let duplicate = !out.insert(field.clone());
        if duplicate {
            out.insert(format!("__cassie_ambiguous__.{field}"));
        }
        for qualifier in &qualifiers {
            out.insert(format!("{qualifier}.{field}"));
            if duplicate {
                out.insert(format!("__cassie_ambiguous__.{qualifier}.{field}"));
            }
        }
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
            }
            | SelectItem::Expr {
                alias: Some(alias), ..
            } => {
                aliases.insert(crate::sql::ColumnIdentifierPath::stored_field_key(alias));
            }
            _ => {}
        }
    }
    aliases
}

pub(super) fn validate_functions(
    statement: &SelectStatement,
    catalog: &Catalog,
    context: &super::BindingContext,
) -> Result<(), CassieError> {
    let mut seen = Vec::new();
    collect_functions(statement, &mut seen);
    validate_function_calls(seen, catalog, context)
}

pub(super) use super::function_validation::validate_function_calls;

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
        (Expr::Column(left), Expr::Column(right)) => left == right,
        _ => format!("{left:?}") == format!("{right:?}"),
    }
}

pub(super) fn collect_functions(statement: &SelectStatement, out: &mut Vec<FunctionCall>) {
    collect_source_functions(&statement.source, out);
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
            CteQuery::Recursive {
                base, recursive, ..
            } => {
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

fn collect_source_functions(source: &QuerySource, out: &mut Vec<FunctionCall>) {
    match source {
        QuerySource::Values { rows } => {
            for expr in rows.iter().flatten() {
                collect_expr(expr, out);
            }
        }

        QuerySource::Aliased { source, .. } => collect_source_functions(source, out),
        QuerySource::TableFunction { function, .. } => {
            if !crate::sql::functions::is_catalog_table(&function.name) {
                out.push(function.clone());
            }
            for arg in function.expressions() {
                collect_expr(arg, out);
            }
        }
        QuerySource::Subquery { select, .. } => collect_functions(select, out),
        QuerySource::Join {
            left, right, on, ..
        } => {
            collect_source_functions(left, out);
            collect_source_functions(right, out);
            collect_expr(on, out);
        }
        QuerySource::Collection(_) | QuerySource::Cte(_) | QuerySource::SingleRow => {}
    }
}

pub(super) fn collect_item(item: &SelectItem, out: &mut Vec<FunctionCall>) {
    match item {
        SelectItem::Function { function, .. } => {
            out.push(function.clone());
            for arg in function.expressions() {
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
    if let Expr::RowCompare(comparison) = expr {
        for field in comparison.left.iter().chain(&comparison.right) {
            collect_expr(field, out);
        }
    }
    if let Expr::Function(function) = expr {
        out.push((**function).clone());
        for arg in function.expressions() {
            collect_expr(arg, out);
        }
    }
    if let Expr::Binary { left, right, .. } = expr {
        collect_expr(left, out);
        collect_expr(right, out);
    }
    if let Expr::IsNull { expr, .. } | Expr::TruthTest { expr, .. } = expr {
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
    }
    | Expr::Like {
        expr,
        pattern: low,
        escape: high,
        ..
    } = expr
    {
        collect_expr(expr, out);
        collect_expr(low, out);
        collect_expr(high, out);
    }
    if let Expr::Cast { expr, .. }
    | Expr::UnaryNumeric { expr, .. }
    | Expr::IntegerNegate { expr, .. } = expr
    {
        collect_expr(expr, out);
    }
    if let Expr::Not { expr } = expr {
        collect_expr(expr, out);
    }
}

pub(super) fn recursive_cte_references_self(statement: &ParsedStatement, cte_name: &str) -> bool {
    recursive_cte_reference_count(statement, cte_name) > 0
}

pub(super) fn recursive_cte_reference_count(statement: &ParsedStatement, cte_name: &str) -> usize {
    match &statement.statement {
        QueryStatement::Select(select) => source_cte_reference_count(&select.source, cte_name),
        _ => 0,
    }
}

fn source_cte_reference_count(source: &QuerySource, cte_name: &str) -> usize {
    match source {
        QuerySource::Aliased { source, .. } => source_cte_reference_count(source, cte_name),
        QuerySource::Cte(name) => usize::from(name.eq_ignore_ascii_case(cte_name)),
        QuerySource::Collection(name) => usize::from(name.as_str().eq_ignore_ascii_case(cte_name)),
        QuerySource::TableFunction { .. } | QuerySource::Values { .. } | QuerySource::SingleRow => {
            0
        }
        QuerySource::Subquery { select, .. } => {
            source_cte_reference_count(&select.source, cte_name)
        }
        QuerySource::Join { left, right, .. } => {
            source_cte_reference_count(left, cte_name) + source_cte_reference_count(right, cte_name)
        }
    }
}

pub(super) use super::references::{
    validate_expression, validate_expression_in_scope, validate_expression_references,
    validate_order_by_references, validate_projection_references,
};
