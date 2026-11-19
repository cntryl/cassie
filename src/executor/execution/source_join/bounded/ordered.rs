//! Stream the proven source in result order, probing each unique dimension in SQL order.
use super::{
    check_timeout, combine_rows, filter, project_source_row, BatchRow, JoinRetentionContext,
    JoinRetentionPhase, QueryError, SourceExecutionEnv, SourceRowShape, Value,
};
use crate::executor::execution::index_read::ordered_join::{
    prepare, OrderedJoin, Relation, STRATEGY,
};
use crate::executor::execution::{projected_read, LogicalPlan, PlanExecutionEnv};
use crate::midge::adapter::{DocumentRef, ScalarIndexScanRequest};
use crate::runtime::{accounted::AccountedVec, OrderedJoinCounts};
use std::sync::Arc;

impl crate::app::Cassie {
    pub(in crate::executor::execution) fn execute_ordered_join(
        &self,
        plan_env: &PlanExecutionEnv<'_>,
        plan: &LogicalPlan,
    ) -> Result<Option<Vec<BatchRow>>, QueryError> {
        let Some(join) = prepare(self, plan_env.session, plan)? else {
            return Ok(None);
        };
        let env = crate::executor::execution::source::source_execution_env_with_parameter_oids(
            self,
            plan_env.session,
            plan_env.user_functions,
            plan_env.params,
            plan_env.parameter_type_oids,
            plan_env.controls,
        );
        let mut output = AccountedVec::try_new(env.controls)?;
        let mut counts = OrderedJoinCounts::default();
        let mut stages = vec![Stage::default(); join.probes.len()];
        let mut consume = |document: DocumentRef| {
            let row = project(&env, &join.source, &document)?;
            counts.order_rows += 1;
            if let Some(row) = probe(&env, &join, row, &mut stages, &mut counts)? {
                if output.len() >= env.controls.max_result_rows {
                    return Err(crate::app::CassieError::ResourceLimit(format!(
                        "query result row limit exceeded: {} > {}",
                        output.len().saturating_add(1),
                        env.controls.max_result_rows
                    ))
                    .into());
                }
                output.try_push_with(0, || row)?;
            }
            Ok::<_, QueryError>(output.len() < join.limit)
        };
        let entries = if join.timestamp_fields.is_empty() || join.limit == 0 {
            let mut visited = 0;
            self.midge.visit_scalar_index_documents(
                &join.index,
                &join.request,
                env.controls,
                |doc| {
                    visited += 1;
                    consume(doc)
                },
            )?;
            visited
        } else {
            let Some(hits) = crate::executor::execution::index_read::key_order::timestamp_page(
                self,
                &join.index,
                &join.request,
                &join.timestamp_fields,
                join.limit,
                env.controls,
            )?
            else {
                return Ok(None);
            };
            let (hits, _memory) = hits.into_parts();
            let entries = hits.len();
            for hit in hits {
                check_timeout(env.controls)?;
                if !consume(DocumentRef {
                    id: hit.id,
                    payload: serde_json::Value::Object(hit.fields),
                })? {
                    break;
                }
            }
            entries
        };
        counts.order_index_entries = entries;
        let (rows, _slots) = output.into_parts();
        let mut batches = vec![rows];
        let rows = projected_read::finalize_projected_filtered_read(
            projected_read::ProjectedReadFinalization {
                cassie: self,
                session: env.session,
                plan,
                user_functions: env.user_functions,
                params: env.params,
                controls: env.controls,
                apply_filter: false,
                apply_sort: false,
                index_usage: None,
            },
            &mut batches,
        )?;
        check_timeout(env.controls)?;
        counts.joined_rows = rows.len();
        self.runtime.record_read_path_ordered_bounded_scan(
            &join.source.collection,
            counts.order_index_entries,
            &join.index.name,
        );
        for stage in stages {
            self.runtime.record_join_execution(
                STRATEGY,
                stage.input,
                stage.hits,
                stage.hits,
                stage.hits,
                None,
            );
        }
        env.controls.record_ordered_join(counts);
        Ok(Some(rows))
    }
}

#[derive(Default, Clone)]
struct Stage {
    input: usize,
    hits: usize,
}

fn project(
    env: &SourceExecutionEnv<'_>,
    relation: &Relation,
    doc: &DocumentRef,
) -> Result<BatchRow, QueryError> {
    project_source_row(
        env,
        doc,
        SourceRowShape {
            collection: &relation.collection,
            fields: &relation.fields,
            schema: Some(&relation.schema),
        },
        JoinRetentionPhase::BoundedSource,
        &JoinRetentionContext::default(),
    )
}

fn probe(
    env: &SourceExecutionEnv<'_>,
    join: &OrderedJoin,
    mut row: BatchRow,
    stages: &mut [Stage],
    counts: &mut OrderedJoinCounts,
) -> Result<Option<BatchRow>, QueryError> {
    for (probe, stage) in join.probes.iter().zip(stages) {
        check_timeout(env.controls)?;
        stage.input += 1;
        let Some(Value::Int64(key)) = row.get(&probe.input) else {
            return Ok(None);
        };
        let request = ScalarIndexScanRequest {
            equality_prefix: vec![serde_json::Value::from(*key)],
            limit: Some(1),
            ..Default::default()
        };
        counts.dimension_probes += 1;
        let hits =
            env.cassie
                .midge
                .scan_scalar_index_controlled(&probe.index, &request, env.controls)?;
        env.cassie.runtime.record_read_path_index_seek(
            &probe.relation.collection,
            hits.len(),
            &probe.index.name,
        );
        let Some(hit) = hits.as_slice().first() else {
            return Ok(None);
        };
        counts.dimension_base_fetches += 1;
        let Some(source) = env.cassie.midge.get_retrieval_document_controlled(
            &probe.relation.collection,
            &hit.id,
            env.controls,
        )?
        else {
            return Ok(None);
        };
        let (document, _read_memory) = source.into_parts();
        let right = project(env, &probe.relation, &document)?;
        let memory = env
            .controls
            .reserve_query_memory(super::super::accounting::combined_row_bytes(&row, &right)?)?;
        let mut combined = combine_rows(&row, &right)?;
        combined.attach_operator_memory(env.controls, Arc::new(memory))?;
        row = combined;
        stage.hits += 1;
        counts.intermediate_rows += 1;
    }
    counts.fully_joined_rows += 1;
    for residual in &join.residuals {
        if !filter::eval_scalar(
            &row,
            residual,
            env.params,
            None,
            env.user_functions,
            None,
            env.session,
        )?
        .is_true()?
        {
            return Ok(None);
        }
    }
    Ok(Some(row))
}
