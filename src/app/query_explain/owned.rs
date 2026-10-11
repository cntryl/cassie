use std::mem::{align_of, size_of};
use std::sync::Arc;

use serde::ser::SerializeStruct;

use crate::app::CassieError;
use crate::executor::retained_memory::{add, mul};
use crate::executor::QueryResult;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

use super::allocation::PlanAllocation;
use super::{
    QueryPlanAnalyze, QueryPlanAttribute, QueryPlanDiagnostics, QueryPlanEstimates,
    QueryPlanFeature, QueryPlanNode, QueryPlanSummary,
};

#[derive(Debug, Clone, serde::Serialize)]
pub struct QueryExplainOutput {
    result: QueryResult,
    plan: QueryExplainPlan,
}

impl QueryExplainOutput {
    pub(crate) fn new(result: QueryResult, plan: QueryExplainPlan) -> Self {
        Self { result, plan }
    }

    #[must_use]
    pub fn result(&self) -> &QueryResult {
        &self.result
    }

    #[must_use]
    pub fn plan(&self) -> &QueryExplainPlan {
        &self.plan
    }

    #[must_use]
    pub fn into_parts(self) -> (QueryResult, QueryExplainPlan) {
        (self.result, self.plan)
    }
}

#[derive(Debug, Clone)]
pub struct QueryExplainPlan {
    base: Arc<OwnedPlan>,
    analyze: Option<Arc<OwnedAnalyze>>,
}

#[derive(Debug)]
struct OwnedPlan {
    data: PlanData,
    _memory: QueryMemoryReservation,
}

#[derive(Debug)]
struct OwnedAnalyze {
    data: QueryPlanAnalyze,
    _memory: QueryMemoryReservation,
}

#[derive(Debug)]
pub(super) struct PlanData {
    pub(super) format_version: u32,
    pub(super) summary: QueryPlanSummary,
    pub(super) nodes: Vec<QueryPlanNode>,
    pub(super) attributes: Vec<QueryPlanAttribute>,
    pub(super) estimates: QueryPlanEstimates,
    pub(super) features: Vec<QueryPlanFeature>,
    pub(super) diagnostics: QueryPlanDiagnostics,
}

impl QueryExplainPlan {
    pub(super) fn try_build(
        controls: &QueryExecutionControls,
        build: impl FnOnce(&mut PlanAllocation<'_>) -> Result<PlanData, CassieError>,
    ) -> Result<Self, CassieError> {
        let mut allocation = PlanAllocation::new(controls, arc_bytes::<OwnedPlan>()?)?;
        let data = build(&mut allocation)?;
        let retained = add(arc_bytes::<OwnedPlan>()?, plan_heap_bytes(&data)?)?;
        allocation.finish_retained(retained)?;
        let memory = allocation.into_memory();
        Ok(Self {
            base: Arc::new(OwnedPlan {
                data,
                _memory: memory,
            }),
            analyze: None,
        })
    }

    pub(crate) fn try_with_analyze_with(
        &self,
        controls: &QueryExecutionControls,
        build: impl FnOnce(&mut PlanAllocation<'_>) -> Result<QueryPlanAnalyze, CassieError>,
    ) -> Result<Self, CassieError> {
        let mut allocation = PlanAllocation::new(controls, arc_bytes::<OwnedAnalyze>()?)?;
        let data = build(&mut allocation)?;
        let retained = add(arc_bytes::<OwnedAnalyze>()?, analyze_heap_bytes(&data)?)?;
        allocation.finish_retained(retained)?;
        let memory = allocation.into_memory();
        Ok(Self {
            base: Arc::clone(&self.base),
            analyze: Some(Arc::new(OwnedAnalyze {
                data,
                _memory: memory,
            })),
        })
    }

    #[must_use]
    pub fn format_version(&self) -> u32 {
        self.base.data.format_version
    }

    #[must_use]
    pub fn summary(&self) -> &QueryPlanSummary {
        &self.base.data.summary
    }

    #[must_use]
    pub fn nodes(&self) -> &[QueryPlanNode] {
        &self.base.data.nodes
    }

    #[must_use]
    pub fn attributes(&self) -> &[QueryPlanAttribute] {
        &self.base.data.attributes
    }

    #[must_use]
    pub fn estimates(&self) -> &QueryPlanEstimates {
        &self.base.data.estimates
    }

    #[must_use]
    pub fn features(&self) -> &[QueryPlanFeature] {
        &self.base.data.features
    }

    #[must_use]
    pub fn diagnostics(&self) -> &QueryPlanDiagnostics {
        &self.base.data.diagnostics
    }

    #[must_use]
    pub fn analyze(&self) -> Option<&QueryPlanAnalyze> {
        self.analyze.as_ref().map(|owned| &owned.data)
    }
}

impl serde::Serialize for QueryExplainPlan {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut output = serializer
            .serialize_struct("QueryExplainPlan", 7 + usize::from(self.analyze.is_some()))?;
        output.serialize_field("format_version", &self.format_version())?;
        output.serialize_field("summary", self.summary())?;
        output.serialize_field("nodes", self.nodes())?;
        output.serialize_field("attributes", self.attributes())?;
        output.serialize_field("estimates", self.estimates())?;
        output.serialize_field("features", self.features())?;
        output.serialize_field("diagnostics", self.diagnostics())?;
        if let Some(analyze) = self.analyze() {
            output.serialize_field("analyze", analyze)?;
        }
        output.end()
    }
}

fn arc_bytes<T>() -> Result<usize, CassieError> {
    let alignment = align_of::<T>();
    let header = add(2 * size_of::<usize>(), alignment - 1)? / alignment * alignment;
    add(header, size_of::<T>())
}

fn strings_heap<'a>(values: impl IntoIterator<Item = &'a String>) -> Result<usize, CassieError> {
    values
        .into_iter()
        .try_fold(0, |bytes, value| add(bytes, value.capacity()))
}

fn string_vector_heap(values: &[String], capacity: usize) -> Result<usize, CassieError> {
    add(mul(capacity, size_of::<String>())?, strings_heap(values)?)
}

fn plan_heap_bytes(data: &PlanData) -> Result<usize, CassieError> {
    let summary = &data.summary;
    let mut bytes = strings_heap([
        &summary.collection,
        &summary.root_operator,
        &summary.access_path,
        &summary.storage_mode,
    ])?;
    if let Some(index) = &summary.selected_index {
        bytes = add(bytes, index.capacity())?;
    }
    bytes = add(
        bytes,
        mul(data.nodes.capacity(), size_of::<QueryPlanNode>())?,
    )?;
    for node in &data.nodes {
        bytes = add(bytes, node_heap_bytes(node)?)?;
    }
    bytes = add(
        bytes,
        mul(data.attributes.capacity(), size_of::<QueryPlanAttribute>())?,
    )?;
    for attribute in &data.attributes {
        bytes = add(
            bytes,
            strings_heap([&attribute.label, &attribute.value, &attribute.intent])?,
        )?;
    }
    bytes = add(bytes, data.estimates.cost_source.capacity())?;
    bytes = add(
        bytes,
        string_vector_heap(
            &data.estimates.rejected_alternatives,
            data.estimates.rejected_alternatives.capacity(),
        )?,
    )?;
    bytes = add(
        bytes,
        mul(data.features.capacity(), size_of::<QueryPlanFeature>())?,
    )?;
    for feature in &data.features {
        bytes = add(
            bytes,
            strings_heap([
                &feature.id,
                &feature.label,
                &feature.intent,
                &feature.detail,
                &feature.node_id,
            ])?,
        )?;
    }
    let diagnostics = &data.diagnostics;
    bytes = add(
        bytes,
        strings_heap([
            &diagnostics.access_path_reason,
            &diagnostics.fallback_reason,
            &diagnostics.pagination_strategy,
            &diagnostics.early_stop,
            &diagnostics.projection_shape,
            &diagnostics.operator_feedback_state,
            &diagnostics.operator_feedback_reason,
            &diagnostics.adaptive_decision_point,
            &diagnostics.adaptive_selected_alternative,
            &diagnostics.adaptive_reason,
            &diagnostics.join_strategy,
            &diagnostics.join_fallback_reason,
            &diagnostics.rollup_rewrite,
            &diagnostics.projection_freshness,
        ])?,
    )?;
    add(
        bytes,
        string_vector_heap(
            &diagnostics.adaptive_candidates,
            diagnostics.adaptive_candidates.capacity(),
        )?,
    )
}

fn node_heap_bytes(node: &QueryPlanNode) -> Result<usize, CassieError> {
    let mut bytes = strings_heap([
        &node.id,
        &node.label,
        &node.kind,
        &node.detail,
        &node.status,
    ])?;
    bytes = add(
        bytes,
        string_vector_heap(&node.badges, node.badges.capacity())?,
    )?;
    bytes = add(
        bytes,
        mul(node.metrics.capacity(), size_of::<super::QueryPlanMetric>())?,
    )?;
    for metric in &node.metrics {
        bytes = add(bytes, strings_heap([&metric.label, &metric.value])?)?;
        if let Some(unit) = &metric.unit {
            bytes = add(bytes, unit.capacity())?;
        }
    }
    Ok(bytes)
}

fn analyze_heap_bytes(data: &QueryPlanAnalyze) -> Result<usize, CassieError> {
    add(
        mul(
            data.operator_actuals.capacity(),
            size_of::<super::QueryPlanOperatorActual>(),
        )?,
        strings_heap(data.operator_actuals.iter().map(|actual| &actual.operator))?,
    )
}
