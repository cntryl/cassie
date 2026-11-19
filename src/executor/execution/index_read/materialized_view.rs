//! Unique point reads through a SQL materialized view's published output.
use super::{
    execute_scalar_spec, projected_read, selected_scalar_spec, BatchRow, Cassie, Expr, IndexMeta,
    LogicalPlan, QueryError, ScalarIndexPlanPath,
};
use crate::executor::execution::source::SourceExecutionEnv;
use crate::sql::ast::{BinaryOp, QuerySource};
use crate::sql::ColumnIdentifierPath;

fn lookup_field(plan: &LogicalPlan) -> Option<String> {
    if !matches!(plan.source, QuerySource::Collection(_)) || !plan.order.is_empty() {
        return None;
    }
    let Expr::Binary {
        left,
        op: BinaryOp::Eq,
        right,
    } = plan.filter.as_ref()?
    else {
        return None;
    };
    let (field, value) = match (left.as_ref(), right.as_ref()) {
        (Expr::Column(field), value) | (value, Expr::Column(field)) => (field, value),
        _ => return None,
    };
    matches!(
        value,
        Expr::Param(_)
            | Expr::IntegerLiteral(_)
            | Expr::StringLiteral(_)
            | Expr::BoolLiteral(_)
            | Expr::NumberLiteral(_)
    )
    .then(|| ColumnIdentifierPath::reference_field_key(field))
}

fn unique_point_index(cassie: &Cassie, output: &str, field: &str) -> Option<IndexMeta> {
    cassie.catalog.list_indexes(output).into_iter().find(|index| {
        index.collection == output && index.unique && index.kind == crate::catalog::IndexKind::Scalar
            && index.expressions.is_empty() && index.predicate.is_none()
            && matches!(index.normalized_fields().as_slice(), [key] if ColumnIdentifierPath::stored_field_key(key) == field)
    })
}

impl Cassie {
    /// Only the logical name is retained in a cached plan. Execute binds the version anew.
    pub(crate) fn logical_materialized_view_index(&self, plan: &LogicalPlan) -> Option<IndexMeta> {
        let field = lookup_field(plan)?;
        let output = self
            .catalog
            .materialized_view_output_collection(&plan.collection)?;
        projected_read::projected_filtered_read_spec(plan)?;
        let mut index = unique_point_index(self, &output, &field)?;
        index.collection.clone_from(&plan.collection);
        Some(index)
    }
}

pub(in crate::executor::execution) fn execute(
    env: &SourceExecutionEnv<'_>,
    plan: &LogicalPlan,
) -> Result<Option<Vec<BatchRow>>, QueryError> {
    let Some(field) = lookup_field(plan) else {
        return Ok(None);
    };
    if !env
        .cassie
        .catalog
        .is_sql_materialized_view(&plan.collection)
    {
        return Ok(None);
    }
    let Some(mut projected) = projected_read::projected_filtered_read_spec(plan) else {
        return Ok(None);
    };
    let Some(publication) = env
        .cassie
        .midge
        .statement_materialized_view(&plan.collection)?
    else {
        return Ok(None);
    };
    let Some(definition) = publication
        .materialized
        .as_ref()
        .filter(|d| d.is_sql_materialized_view())
    else {
        return Ok(None);
    };
    crate::sql::definition_guard::projection(definition)?;
    let Some(output) = publication.active_output_collection() else {
        return Ok(None);
    };
    let _read = crate::midge::adapter::StatementDataRead::enter_materialized_output(
        env.controls,
        &plan.collection,
        output,
    );
    let Some(candidate) = unique_point_index(env.cassie, output, &field) else {
        return Ok(None);
    };
    let Some(index) = env
        .cassie
        .midge
        .published_scalar_index(output, &candidate)?
    else {
        return Ok(None);
    };
    if env
        .session
        .is_some_and(|s| s.has_collection_changes(output))
    {
        return Ok(None);
    }
    projected.collection = output.to_owned();
    let Some(spec) =
        selected_scalar_spec(env.cassie, projected, plan, env.params, index, false, false)?
    else {
        return Ok(None);
    };
    if spec.path != ScalarIndexPlanPath::IndexSeek {
        return Ok(None);
    }
    execute_scalar_spec(
        env.cassie,
        env.session,
        plan,
        env.user_functions,
        env.params,
        env.controls,
        spec,
    )
}
