use super::select::{cte_output_fields, validate_recursive_cte_shape};
use super::{
    bind_statement, recursive_cte_reference_count, recursive_cte_references_self, CassieError,
    Catalog, CteQuery, CteScope,
};

pub(super) fn bind_recursive_cte_query(
    query: CteQuery,
    declared_aliases: &[String],
    catalog: &Catalog,
    outer_scope: &CteScope,
    cte_name: &str,
    context: &super::context::BindingContext,
) -> Result<CteQuery, CassieError> {
    let CteQuery::Recursive {
        operator,
        base,
        recursive,
    } = query
    else {
        return Err(CassieError::Planner(
            "recursive CTE binding received a non-recursive query".into(),
        ));
    };
    let cte_name_lc = cte_name.to_ascii_lowercase();
    if recursive_cte_references_self(base.as_ref(), cte_name) {
        return Err(CassieError::Planner(format!(
            "recursive CTE '{cte_name}' anchor cannot reference itself"
        )));
    }

    let recursive_aliases = if declared_aliases.is_empty() {
        cte_output_fields(&CteQuery::Simple(base.clone()))?
    } else {
        declared_aliases.to_vec()
    };
    if recursive_aliases.is_empty() || recursive_aliases.iter().any(|alias| alias == "*") {
        return Err(CassieError::Planner(format!(
            "recursive CTE '{cte_name}' requires named output columns"
        )));
    }
    let mut recursive_scope = outer_scope.clone();
    recursive_scope.insert(cte_name_lc, recursive_aliases.clone());

    let bound_base = bind_statement(*base, catalog, outer_scope, context)?;
    let bound_recursive = bind_statement(*recursive, catalog, &recursive_scope, context)?;

    let reference_count = recursive_cte_reference_count(&bound_recursive, cte_name);
    if reference_count == 0 {
        return Err(CassieError::Planner(format!(
            "recursive CTE '{cte_name}' must reference itself in recursive term"
        )));
    }
    if reference_count > 1 {
        return Err(CassieError::Planner(format!(
            "recursive CTE '{cte_name}' has unsupported multiple recursive references"
        )));
    }
    validate_recursive_cte_shape(
        &bound_base,
        &bound_recursive,
        catalog,
        outer_scope,
        cte_name,
        &recursive_aliases,
    )?;

    Ok(CteQuery::Recursive {
        operator,
        base: Box::new(bound_base),
        recursive: Box::new(bound_recursive),
    })
}
