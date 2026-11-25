use super::{
    normalize_relation_name, BindingContext, CallProcedureStatement, CassieError, Catalog,
    CreateFunctionStatement, CreateProcedureStatement, DropFunctionStatement,
    DropProcedureStatement, Expr, HashSet, QueryStatement,
};

pub(super) fn bind_create_function(
    mut statement: CreateFunctionStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<CreateFunctionStatement, CassieError> {
    let requested_name = statement.name.trim().to_owned();
    let name = super::normalize_new_relation_name(statement.name.trim(), context, catalog)?;
    if name.is_empty() {
        return Err(CassieError::Planner(
            "CREATE FUNCTION requires a name".into(),
        ));
    }

    let builtin_name = crate::sql::functions::registry()
        .iter()
        .any(|function| function.name.eq_ignore_ascii_case(&name));
    let distinct_width_bucket_arity =
        crate::sql::functions::function(&name).is_some_and(|function| {
            function.id
                == crate::sql::functions::FunctionId::Math(
                    crate::sql::functions::MathFunction::WidthBucket,
                )
                && !function.arity.matches(statement.args.len())
        });
    if builtin_name && !distinct_width_bucket_arity {
        return Err(CassieError::Planner(format!(
            "cannot create function '{name}' because it conflicts with built-in function"
        )));
    }

    super::routine_replace::check_clauses(
        "FUNCTION",
        statement.or_replace,
        statement.if_not_exists,
    )?;
    if !statement.if_not_exists && !statement.or_replace && catalog.get_function(&name).is_some() {
        return Err(super::routine_replace::duplicate("function", &name));
    }

    let mut seen = HashSet::new();
    for arg in &mut statement.args {
        let arg_name = arg.name.trim().to_string();
        if arg_name.is_empty() {
            return Err(CassieError::Planner(
                "function argument name cannot be empty".into(),
            ));
        }
        let key = crate::sql::ColumnIdentifierPath::reference_field_key(&arg_name);
        if !seen.insert(key) {
            return Err(CassieError::Planner(format!(
                "function '{name}' has duplicate argument '{arg_name}'"
            )));
        }
        arg.name = arg_name;
    }
    if statement.or_replace {
        super::routine_replace::check_function_replace(
            catalog,
            &name,
            &statement.args,
            &statement.return_type,
        )?;
    }

    let body = statement.body.trim().to_string();
    if body.is_empty() {
        return Err(CassieError::Planner(
            "CREATE FUNCTION requires a body".into(),
        ));
    }
    let parsed_body = crate::sql::parser::parse_expression(&body).map_err(|error| {
        let message = format!("invalid function body for '{name}': {error}");
        if error.kind() == crate::sql::SqlErrorKind::ResourceLimit {
            CassieError::ResourceLimit(message)
        } else {
            CassieError::Planner(message)
        }
    })?;

    crate::sql::definition_guard::legacy_expression(&parsed_body)?;

    let argument_names = statement
        .args
        .iter()
        .map(|arg| crate::sql::ColumnIdentifierPath::reference_field_key(&arg.name))
        .collect::<HashSet<_>>();
    let scope = super::CteScope::new(None, &[]);
    super::validate_expression_in_scope(
        &parsed_body,
        &argument_names,
        &HashSet::new(),
        false,
        super::references::ReferenceScope::new(&scope, catalog, context),
    )
    .map_err(|error| match error {
        CassieError::UndefinedColumn(column) => {
            crate::app::DiagnosticError::new("42703", format!("column \"{column}\" does not exist"))
                .into()
        }
        error => error,
    })?;

    if function_body_references(&parsed_body, &name, context) {
        return Err(CassieError::Planner(format!(
            "function '{requested_name}' cannot call itself"
        )));
    }

    statement.name = name;
    statement.body = body;
    Ok(statement)
}

pub(super) fn bind_drop_function(
    mut statement: DropFunctionStatement,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<DropFunctionStatement, CassieError> {
    let name = normalize_relation_name(statement.name.trim(), context)?;
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
    context: &BindingContext,
) -> Result<CreateProcedureStatement, CassieError> {
    let name = super::normalize_new_relation_name(statement.name.trim(), context, catalog)?;
    if name.is_empty() {
        return Err(CassieError::Planner(
            "CREATE PROCEDURE requires a name".into(),
        ));
    }

    super::routine_replace::check_clauses(
        "PROCEDURE",
        statement.or_replace,
        statement.if_not_exists,
    )?;
    if !statement.if_not_exists && !statement.or_replace && catalog.get_procedure(&name).is_some() {
        return Err(super::routine_replace::duplicate("procedure", &name));
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
    if statement.or_replace {
        super::routine_replace::check_procedure_replace(catalog, &name, &statement.args)?;
    }

    let body = statement.body.trim().to_string();
    if body.is_empty() {
        return Err(CassieError::Planner(
            "CREATE PROCEDURE requires a body".into(),
        ));
    }

    let parsed_body = crate::sql::parse_statement(&body).map_err(|error| {
        CassieError::Planner(format!("invalid procedure body for '{name}': {error}"))
    })?;
    crate::sql::definition_guard::legacy_statement(&parsed_body)?;

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
    context: &BindingContext,
) -> Result<DropProcedureStatement, CassieError> {
    let name = normalize_relation_name(statement.name.trim(), context)?;
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
    context: &BindingContext,
) -> Result<CallProcedureStatement, CassieError> {
    let name = normalize_relation_name(statement.name.trim(), context)?;
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

    crate::sql::definition_guard::legacy_query_sql(&metadata.body)?;
    let mut bound = statement;
    bound.name = name;
    Ok(bound)
}

pub(super) fn function_body_references(
    expr: &Expr,
    function_name: &str,
    context: &BindingContext,
) -> bool {
    match expr {
        Expr::Function(function) => {
            normalize_relation_name(&function.name, context)
                .is_ok_and(|name| name.eq_ignore_ascii_case(function_name))
                || function.name.eq_ignore_ascii_case(function_name)
                || function
                    .args
                    .iter()
                    .any(|arg| function_body_references(arg, function_name, context))
        }
        Expr::Exists(_) => false,
        _ => expr.any_child(|child| function_body_references(child, function_name, context)),
    }
}
