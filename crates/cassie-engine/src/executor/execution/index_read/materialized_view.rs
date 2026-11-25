//! Unique point reads through a SQL materialized view's published output.
use super::{
    execute_scalar_spec, projected_read, selected_scalar_spec, BatchRow, Cassie, Expr, IndexMeta,
    LogicalPlan, QueryError, ScalarIndexPlanPath,
};
use crate::executor::execution::source::SourceExecutionEnv;
use crate::sql::ast::{BinaryOp, QuerySource};
use crate::sql::ColumnIdentifierPath;

fn lookup_field(
    plan: &LogicalPlan,
    params: Option<&[crate::types::Value]>,
    parameter_types: &[i32],
) -> Option<String> {
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
    let ((Expr::Column(field), value) | (value, Expr::Column(field))) =
        (left.as_ref(), right.as_ref())
    else {
        return None;
    };
    let exact = match value {
        Expr::IntegerLiteral(_) => true,
        Expr::Param(index) => {
            parameter_types
                .get(*index)
                .is_none_or(|oid| matches!(oid, 0 | 20 | 21 | 23))
                && params.is_none_or(|values| {
                    matches!(values.get(*index), Some(crate::types::Value::Int64(_)))
                })
        }
        _ => false,
    };
    exact.then(|| ColumnIdentifierPath::reference_field_key(field))
}

fn unique_point_index(cassie: &Cassie, output: &str, field: &str) -> Option<IndexMeta> {
    if !matches!(
        cassie.catalog.field_type(output, field),
        Some(
            crate::types::DataType::SmallInt
                | crate::types::DataType::Int
                | crate::types::DataType::BigInt
        )
    ) {
        return None;
    }
    cassie.catalog.list_indexes(output).into_iter().find(|index| {
        index.collection == output && index.unique && index.kind == crate::catalog::IndexKind::Scalar
            && index.expressions.is_empty() && index.predicate.is_none()
            && matches!(index.normalized_fields().as_slice(), [key] if ColumnIdentifierPath::stored_field_key(key) == field)
    })
}

impl Cassie {
    /// Simple projected heap/view reads have no transitive SQL sources.
    /// Other query shapes conservatively retain the entire private directory.
    pub(crate) fn direct_materialized_read_source(&self, plan: &LogicalPlan) -> Option<String> {
        if !matches!(plan.source, QuerySource::Collection(_))
            || plan.limit.is_some()
            || plan.offset.is_some()
            || self.catalog.get_view(&plan.collection).is_some()
        {
            return None;
        }
        projected_read::projected_filtered_read_spec(plan).map(|spec| spec.collection)
    }

    /// Only the logical name is retained in a cached plan. Execute binds the version anew.
    pub(crate) fn logical_materialized_view_index(
        &self,
        plan: &LogicalPlan,
        parameter_types: &[i32],
    ) -> Option<IndexMeta> {
        let field = lookup_field(plan, None, parameter_types)?;
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
    let Some(field) = lookup_field(plan, Some(env.params), env.parameter_type_oids) else {
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
        .filter(|metadata| crate::catalog::is_sql_materialized_view(metadata))
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
    output.clone_into(&mut projected.collection);
    let covered = crate::planner::physical::plan_is_covered_by_index(plan, &index);
    let Some(spec) = selected_scalar_spec(
        env.cassie, projected, plan, env.params, index, covered, false,
    )?
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
        &spec,
    )
}
