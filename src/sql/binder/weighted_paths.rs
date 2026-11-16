//! Binding of the `graph_weighted_paths` table function (#1258): its arity, argument
//! expressions and result column types. Graph data is never read while binding.

use super::{BindingContext, CassieError, Catalog, DataType, HashSet, QuerySource};
use crate::sql::ast::{FunctionCall, TableFunctionForm};
use crate::sql::functions::graph_weighted::{self, ARGUMENTS};

/// Checks the call has all nine positional arguments the parser produces.
pub(super) fn validate(
    function: &FunctionCall,
    lateral_fields: &HashSet<String>,
) -> Result<(), CassieError> {
    if function.args.len() != ARGUMENTS.len() || function.has_modifiers() {
        return Err(crate::app::DiagnosticError::new(
            "42883",
            format!("function {}(...) does not exist", graph_weighted::NAME),
        )
        .into());
    }
    for argument in &function.args {
        super::validate_expression(argument, lateral_fields, &HashSet::new(), false)?;
    }
    Ok(())
}

/// Records the database's `graph_path` type on the source so its column is typed.
pub(super) fn bind(
    source: QuerySource,
    catalog: &Catalog,
    context: &BindingContext,
) -> QuerySource {
    let QuerySource::TableFunction {
        name,
        function,
        lateral,
        with_ordinality,
        form: _,
    } = source
    else {
        unreachable!("weighted path binding requires a table function");
    };
    let form = catalog
        .graph_descriptors(&context.database)
        .map(|descriptors| {
            let columns = graph_weighted::columns(Some(DataType::Composite(descriptors.path)));
            Box::new(TableFunctionForm::Typed(
                columns
                    .into_iter()
                    .map(|(_, data_type)| data_type)
                    .collect(),
            ))
        });
    QuerySource::TableFunction {
        name,
        function,
        lateral,
        with_ordinality,
        form,
    }
}

/// The output columns of a weighted path source, typed once bound.
pub(super) fn columns(form: Option<&TableFunctionForm>) -> Vec<(String, DataType)> {
    let mut columns = graph_weighted::columns(None);
    if let Some(TableFunctionForm::Typed(types)) = form {
        for (column, data_type) in columns.iter_mut().zip(types) {
            column.1 = data_type.clone();
        }
    }
    columns
}
