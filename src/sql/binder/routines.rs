use super::*;

pub(super) fn bind_create_function(
    mut statement: CreateFunctionStatement,
    catalog: &Catalog,
) -> Result<CreateFunctionStatement, CassieError> {
    let name = statement.name.trim().to_string();
    if name.is_empty() {
        return Err(CassieError::Planner(
            "CREATE FUNCTION requires a name".into(),
        ));
    }

    if crate::sql::functions::registry()
        .iter()
        .any(|function| function.name.eq_ignore_ascii_case(&name))
    {
        return Err(CassieError::Planner(format!(
            "cannot create function '{name}' because it conflicts with built-in function"
        )));
    }

    if !statement.if_not_exists && catalog.get_function(&name).is_some() {
        return Err(CassieError::Planner(format!(
            "function '{name}' already exists"
        )));
    }

    let mut seen = HashSet::new();
    for arg in &mut statement.args {
        let arg_name = arg.name.trim().to_string();
        if arg_name.is_empty() {
            return Err(CassieError::Planner(
                "function argument name cannot be empty".into(),
            ));
        }
        let key = arg_name.to_ascii_lowercase();
        if !seen.insert(key) {
            return Err(CassieError::Planner(format!(
                "function '{name}' has duplicate argument '{arg_name}'"
            )));
        }
        arg.name = arg_name;
    }

    let body = statement.body.trim().to_string();
    if body.is_empty() {
        return Err(CassieError::Planner(
            "CREATE FUNCTION requires a body".into(),
        ));
    }
    let parsed_body = crate::sql::parser::parse_expression(&body).map_err(|error| {
        CassieError::Planner(format!("invalid function body for '{name}': {}", error.0))
    })?;

    if function_body_references(&parsed_body, &name) {
        return Err(CassieError::Planner(format!(
            "function '{name}' cannot call itself"
        )));
    }

    statement.name = name;
    statement.body = body;
    Ok(statement)
}

pub(super) fn bind_drop_function(
    mut statement: DropFunctionStatement,
    catalog: &Catalog,
) -> Result<DropFunctionStatement, CassieError> {
    let name = statement.name.trim().to_string();
    if name.is_empty() {
        return Err(CassieError::Planner("DROP FUNCTION requires a name".into()));
    }

    if !statement.if_exists && catalog.get_function(&name).is_none() {
        return Err(CassieError::Planner(format!(
            "function '{name}' does not exist"
        )));
    }

    statement.name = name;
    Ok(statement)
}

pub(super) fn bind_create_procedure(
    mut statement: CreateProcedureStatement,
    catalog: &Catalog,
) -> Result<CreateProcedureStatement, CassieError> {
    let name = statement.name.trim().to_string();
    if name.is_empty() {
        return Err(CassieError::Planner(
            "CREATE PROCEDURE requires a name".into(),
        ));
    }

    if !statement.if_not_exists && catalog.get_procedure(&name).is_some() {
        return Err(CassieError::Planner(format!(
            "procedure '{name}' already exists"
        )));
    }

    let mut seen = HashSet::new();
    for arg in &mut statement.args {
        let arg_name = arg.name.trim().to_string();
        if arg_name.is_empty() {
            return Err(CassieError::Planner(
                "procedure argument name cannot be empty".into(),
            ));
        }
        let key = arg_name.to_ascii_lowercase();
        if !seen.insert(key) {
            return Err(CassieError::Planner(format!(
                "procedure '{name}' has duplicate argument '{arg_name}'"
            )));
        }
        arg.name = arg_name;
    }

    let body = statement.body.trim().to_string();
    if body.is_empty() {
        return Err(CassieError::Planner(
            "CREATE PROCEDURE requires a body".into(),
        ));
    }

    let parsed_body = crate::sql::parse_statement(&body).map_err(|error| {
        CassieError::Planner(format!("invalid procedure body for '{name}': {}", error.0))
    })?;
    if matches!(parsed_body.statement, QueryStatement::Transaction(_)) {
        return Err(CassieError::Unsupported(
            "transaction control statements inside procedures are not supported in this version"
                .into(),
        ));
    }

    let body_parameter_count = crate::sql::parameter_count(&parsed_body);
    if body_parameter_count > statement.args.len() {
        return Err(CassieError::Planner(format!(
            "procedure '{name}' body references ${body_parameter_count} but only {} args are declared",
            statement.args.len()
        )));
    }

    statement.name = name;
    statement.body = body;
    Ok(statement)
}

pub(super) fn bind_drop_procedure(
    mut statement: DropProcedureStatement,
    catalog: &Catalog,
) -> Result<DropProcedureStatement, CassieError> {
    let name = statement.name.trim().to_string();
    if name.is_empty() {
        return Err(CassieError::Planner(
            "DROP PROCEDURE requires a name".into(),
        ));
    }

    if !statement.if_exists && catalog.get_procedure(&name).is_none() {
        return Err(CassieError::Planner(format!(
            "procedure '{name}' does not exist"
        )));
    }

    statement.name = name;
    Ok(statement)
}

pub(super) fn bind_call_procedure(
    statement: CallProcedureStatement,
    catalog: &Catalog,
) -> Result<CallProcedureStatement, CassieError> {
    let name = statement.name.trim().to_string();
    if name.is_empty() {
        return Err(CassieError::Planner("CALL requires a name".into()));
    }

    let Some(metadata) = catalog.get_procedure(&name) else {
        return Err(CassieError::Planner(format!(
            "procedure '{name}' does not exist"
        )));
    };

    if statement.args.len() != metadata.args.len() {
        return Err(CassieError::Planner(format!(
            "procedure '{}' expects {} args, got {}",
            name,
            metadata.args.len(),
            statement.args.len()
        )));
    }

    let mut bound = statement;
    bound.name = name;
    Ok(bound)
}

pub(super) fn function_body_references(expr: &Expr, function_name: &str) -> bool {
    let normalized = function_name.to_ascii_lowercase();
    match expr {
        Expr::Function(function) => {
            function.name.eq_ignore_ascii_case(&normalized)
                || function
                    .args
                    .iter()
                    .any(|arg| function_body_references(arg, function_name))
        }
        Expr::Binary { left, right, .. } => {
            function_body_references(left, function_name)
                || function_body_references(right, function_name)
        }
        Expr::IsNull { expr, .. } => function_body_references(expr, function_name),
        Expr::InList { expr, values, .. } => {
            function_body_references(expr, function_name)
                || values
                    .iter()
                    .any(|value| function_body_references(value, function_name))
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            function_body_references(expr, function_name)
                || function_body_references(low, function_name)
                || function_body_references(high, function_name)
        }
        Expr::Not { expr } => function_body_references(expr, function_name),
        Expr::Cast { expr, .. } => function_body_references(expr, function_name),
        Expr::Exists(_) => false,
        Expr::StringLiteral(_)
        | Expr::NumberLiteral(_)
        | Expr::BoolLiteral(_)
        | Expr::Param(_)
        | Expr::Column(_)
        | Expr::Null => false,
    }
}
