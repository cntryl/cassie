//! Builtin and SQL routine validation before planning evaluates constants.
use super::{CassieError, Catalog, FunctionCall};

pub(super) fn validate_function_calls(
    functions: Vec<FunctionCall>,
    catalog: &Catalog,
    context: &super::BindingContext,
) -> Result<(), CassieError> {
    let user_functions = crate::catalog::function_resolution::functions_for_scope(
        &catalog.list_functions(),
        &context.database,
        &context.search_path,
        context.scopes_database_objects(),
    );

    for function in functions {
        if function
            .name
            .eq_ignore_ascii_case(crate::sql::ast::ARRAY_CONSTRUCTOR_FUNCTION)
        {
            continue;
        }
        if crate::sql::table_sources::is_generated(&function.name) {
            return Err(CassieError::Unsupported(
                "set-returning functions are selected only as FROM sources".into(),
            ));
        }
        if crate::sql::functions::is_catalog_table(&function.name) {
            return Err(CassieError::Unsupported(
                "record-valued table-function projection remains pending under #793/#817; use FROM"
                    .into(),
            ));
        }
        if function.name.eq_ignore_ascii_case("cast") {
            if function.args.len() != 2 {
                return Err(CassieError::Planner(format!(
                    "function '{}' expects 2 args",
                    function.name
                )));
            }
            continue;
        }
        let lookup = function.name.to_ascii_lowercase();
        if let Some(metadata) = user_functions.get(&lookup).filter(|metadata| {
            function.args.len() == metadata.args.len()
                && !crate::sql::functions::has_distinct_builtin_arity(
                    &function.name,
                    function.args.len(),
                    metadata.args.len(),
                )
        }) {
            crate::sql::definition_guard::legacy_expression_sql(&metadata.body)?;
            continue;
        }
        if let Some(metadata) = crate::sql::functions::function(&function.name) {
            if metadata.bypass_arity_validation {
                continue;
            }
            if !metadata.arity.matches(function.args.len()) {
                return Err(CassieError::Planner(format!(
                    "function '{}' expects {}, got {}",
                    function.name,
                    metadata.arity.describe(),
                    function.args.len()
                )));
            }
            if metadata.id == crate::sql::functions::FunctionId::PgColumnSize {
                crate::sql::functions::datum_size_kind(&function.args[0])?;
            }
            continue;
        }
        if crate::sql::functions::primitive_casts::candidate(&function).is_some()
            || crate::types::extended::operations::is_function_name(&function.name)
            || crate::sql::functions::hashing::is_hash_function(&function.name)
        {
            continue;
        }
        if crate::sql::functions::is_server_file_helper(&function.name) {
            return Err(crate::sql::functions::reject_server_file_access());
        }
        if crate::sql::functions::is_server_control_helper(&function.name) {
            return Err(crate::sql::functions::reject_server_control());
        }
        let Some(metadata) = user_functions.get(&lookup) else {
            return Err(CassieError::Planner(format!(
                "unsupported function '{}'",
                function.name
            )));
        };
        if function.args.len() != metadata.args.len() {
            return Err(CassieError::Planner(format!(
                "function '{}' expects {}, got {}",
                function.name,
                metadata.args.len(),
                function.args.len()
            )));
        }
        crate::sql::definition_guard::legacy_expression_sql(&metadata.body)?;
    }

    Ok(())
}
