use super::allocation::PlanAllocation;
use crate::app::CassieError;
use crate::sql::ast::{CteQuery, Expr, SelectItem};

use super::super::{vector_prefilter_fallback_reason, vector_prefilter_supported, Cassie};
use super::{non_empty_or_none, projection_freshness, selected_cost};

pub(super) fn plan_line(
    cassie: &Cassie,
    physical: &crate::planner::physical::PhysicalPlan,

    allocation: &mut PlanAllocation<'_>,
) -> Result<String, CassieError> {
    let mut sections = allocation.vector(9)?;
    sections.push(read_plan_section(cassie, physical, allocation)?);
    sections.push(cte_section(physical, allocation)?);
    sections.push(operator_feedback_section(physical, allocation)?);
    sections.push(adaptive_plan_section(physical, allocation)?);
    sections.push(phase03_section(cassie, physical, allocation)?);
    sections.push(join_section(physical, allocation)?);
    sections.push(vectorized_join_section(cassie, physical, allocation)?);
    sections.push(acceleration_section(cassie, physical, allocation)?);
    sections.push(cost_section(physical, allocation)?);
    allocation.join(sections.iter().map(String::as_str), " ")
}

fn cte_section(
    physical: &crate::planner::physical::PhysicalPlan,
    allocation: &mut PlanAllocation<'_>,
) -> Result<String, CassieError> {
    let recursive_ctes = allocation.join(
        physical.logical.ctes.iter().filter_map(|cte| {
            matches!(cte.query, CteQuery::Recursive { .. }).then_some(cte.name.as_str())
        }),
        "|",
    )?;
    allocation.format(format_args!(
        "recursive_cte={}",
        if recursive_ctes.is_empty() {
            "none"
        } else {
            recursive_ctes.as_str()
        }
    ))
}

fn read_plan_section(
    cassie: &Cassie,
    physical: &crate::planner::physical::PhysicalPlan,
    allocation: &mut PlanAllocation<'_>,
) -> Result<String, CassieError> {
    let projection_pruning = !physical.read.projected_scan_fields.is_empty();
    let scan_fields = if projection_pruning {
        allocation.join(
            physical
                .read
                .projected_scan_fields
                .iter()
                .map(String::as_str),
            ",",
        )?
    } else {
        allocation.text("all")?
    };
    let scan_limit = optional_number(physical.read.scan_limit, allocation)?;
    let storage_mode = super::storage_mode(cassie, physical);
    let selected_index = physical.read.selected_index.as_deref().unwrap_or("none");
    let operators = operators_description(physical, allocation)?;
    allocation.format(format_args!(
        "collection={} operators={} predicate_pushdown={} projection_pruning={} scan_fields={} limit_pushdown={} scan_limit={} access_path={} access_path_reason={} fallback_reason={} pagination_strategy={} top_k_mode={} early_stop={} projection_shape={} storage_mode={} index_aware={} index={} index_feedback={}",
        physical.collection,
        operators,
        physical.read.predicate_pushdown,
        projection_pruning,
        scan_fields,
        physical.read.scan_limit.is_some(),
        scan_limit,
        physical.read.access_path.as_str(),
        physical.read.access_path_reason.as_str(),
        physical.read.fallback_reason.as_deref().unwrap_or("none"),
        physical.read.pagination_strategy.as_str(),
        physical.top_k.mode.as_str(),
        physical.read.early_stop.as_str(),
        physical.projection.shape.as_str(),
        storage_mode,
        physical.read.selected_index.is_some(),
        selected_index,
        index_feedback(physical)
    ))
}

fn operators_description(
    physical: &crate::planner::physical::PhysicalPlan,
    allocation: &mut PlanAllocation<'_>,
) -> Result<String, CassieError> {
    if physical.operators.is_empty() {
        return allocation.text("Command");
    }
    let mut operators = allocation.vector(physical.operators.len())?;
    for operator in &physical.operators {
        operators.push(allocation.format(format_args!("{operator:?}"))?);
    }
    allocation.join(operators.iter().map(String::as_str), ">")
}

fn index_feedback(physical: &crate::planner::physical::PhysicalPlan) -> &'static str {
    if physical.read.selected_index.is_some() {
        "enabled"
    } else {
        "none"
    }
}

fn operator_feedback_section(
    physical: &crate::planner::physical::PhysicalPlan,
    allocation: &mut PlanAllocation<'_>,
) -> Result<String, CassieError> {
    let feedback = &physical.operator_feedback;
    allocation.format(format_args!(
        "operator_feedback={} operator_feedback_reason={} operator_feedback_base_candidate={} operator_feedback_selected_candidate={} operator_feedback_base_cost={} operator_feedback_adjusted_cost={} operator_feedback_confidence_bps={} operator_feedback_age_ms={} operator_feedback_samples={} operator_feedback_outliers={}",
        non_empty_or_none(&feedback.state),
        non_empty_or_none(&feedback.reason),
        non_empty_or_none(&feedback.base_candidate),
        non_empty_or_none(&feedback.selected_candidate),
        feedback.base_selected_cost,
        feedback.adjusted_selected_cost,
        feedback.confidence_bps,
        feedback.age_ms,
        feedback.samples,
        feedback.outlier_samples
    ))
}

fn adaptive_plan_section(
    physical: &crate::planner::physical::PhysicalPlan,
    allocation: &mut PlanAllocation<'_>,
) -> Result<String, CassieError> {
    let adaptive_plan = &physical.adaptive_plan;
    let adaptive_candidates = if adaptive_plan.candidates.is_empty() {
        allocation.text("none")?
    } else {
        allocation.join(adaptive_plan.candidates.iter().map(String::as_str), "|")?
    };
    allocation.format(format_args!(
        "adaptive_plan_enabled={} adaptive_decision_point={} adaptive_candidates={} adaptive_base_alternative={} adaptive_selected_alternative={} adaptive_guard={} adaptive_guard_passed={} adaptive_reason={} adaptive_diagnostic={}",
        adaptive_plan.enabled,
        non_empty_or_none(&adaptive_plan.decision_point),
        adaptive_candidates,
        non_empty_or_none(&adaptive_plan.base_alternative),
        non_empty_or_none(&adaptive_plan.selected_alternative),
        non_empty_or_none(&adaptive_plan.guard),
        adaptive_plan.guard_passed,
        non_empty_or_none(&adaptive_plan.reason),
        non_empty_or_none(&adaptive_plan.diagnostic)
    ))
}

fn phase03_section(
    cassie: &Cassie,
    physical: &crate::planner::physical::PhysicalPlan,
    allocation: &mut PlanAllocation<'_>,
) -> Result<String, CassieError> {
    let diagnostics = phase03_diagnostics(cassie, physical, allocation)?;
    let encoded_execution =
        physical.read.column_batch_index.is_some() || physical.aggregate.acceleration;
    let codec_policy = if encoded_execution {
        "automatic_min_savings_max_32b_5pct"
    } else {
        "none"
    };
    let top_k_limit = optional_number(physical.top_k.limit, allocation)?;
    let predicate_fields = predicate_fields(physical, allocation)?;
    let projection_fields = projection_fields(physical, allocation)?;
    let prefilter = prefilter_description(cassie, physical, allocation)?;
    let time_series = time_series_description(cassie, physical, allocation)?;
    let time_series_storage = time_series_storage_description(cassie, physical, allocation)?;
    let budget = candidate_budget(cassie, physical, allocation)?;
    allocation.format(format_args!(
        "covered_index={} column_batch_index={} encoded_execution={} automatic_codec_policy={} late_materialization={} predicate_fields={} projection_fields={} column_fallback_reason={} column_native={} hybrid_row_column={} vectorized_aggregate={} parallel_pipeline={} analytical_projection={} prefilter={} time_series={} time_series_storage={} top_k={} top_k_limit={} candidate_budget={}",
        physical.read.covered_index,
        physical.read.column_batch_index.as_deref().unwrap_or("none"),
        encoded_execution,
        codec_policy,
        encoded_execution,
        predicate_fields,
        projection_fields,
        physical.read.fallback_reason.as_deref().unwrap_or("none"),
        diagnostics.column_native,
        diagnostics.hybrid_row_column,
        diagnostics.vectorized_aggregate,
        diagnostics.parallel_pipeline,
        diagnostics.analytical_projection,
        prefilter,
        time_series,
        time_series_storage,
        physical.top_k.enabled,
        top_k_limit,
        budget
    ))
}

fn predicate_fields(
    physical: &crate::planner::physical::PhysicalPlan,
    allocation: &mut PlanAllocation<'_>,
) -> Result<String, CassieError> {
    let mut fields = Vec::new();
    if let Some(filter) = physical.logical.filter.as_ref() {
        collect_expr_columns(filter, &mut fields, allocation)?;
    }
    joined_fields(&mut fields, allocation)
}

fn projection_fields(
    physical: &crate::planner::physical::PhysicalPlan,
    allocation: &mut PlanAllocation<'_>,
) -> Result<String, CassieError> {
    let mut fields = Vec::new();
    for item in &physical.logical.projection {
        match item {
            SelectItem::Wildcard => return allocation.text("all"),
            SelectItem::Column { name, .. } => {
                let field = allocation.field_key(name)?;
                allocation.push(&mut fields, field)?;
            }
            SelectItem::Function { function, .. } => {
                for argument in function.expressions() {
                    collect_expr_columns(argument, &mut fields, allocation)?;
                }
            }
            SelectItem::Expr { expr, .. } => collect_expr_columns(expr, &mut fields, allocation)?,
            SelectItem::WindowFunction { function, .. } => {
                for argument in &function.args {
                    collect_expr_columns(argument, &mut fields, allocation)?;
                }
            }
        }
    }
    joined_fields(&mut fields, allocation)
}

fn collect_expr_columns(
    expr: &Expr,
    fields: &mut Vec<String>,
    allocation: &mut PlanAllocation<'_>,
) -> Result<(), CassieError> {
    allocation.check()?;
    match expr {
        Expr::Column(field) => {
            let field = allocation.field_key(field)?;
            allocation.push(fields, field)?;
        }
        Expr::RowCompare(comparison) => {
            for field in comparison.left.iter().chain(&comparison.right) {
                collect_expr_columns(field, fields, allocation)?;
            }
        }
        Expr::Binary { left, right, .. } => {
            collect_expr_columns(left, fields, allocation)?;
            collect_expr_columns(right, fields, allocation)?;
        }
        Expr::IsNull { expr, .. }
        | Expr::TruthTest { expr, .. }
        | Expr::Not { expr }
        | Expr::Cast { expr, .. }
        | Expr::UnaryNumeric { expr, .. }
        | Expr::IntegerNegate { expr, .. } => {
            collect_expr_columns(expr, fields, allocation)?;
        }
        Expr::Between {
            expr, low, high, ..
        }
        | Expr::Like {
            expr,
            pattern: low,
            escape: high,
        } => {
            collect_expr_columns(expr, fields, allocation)?;
            collect_expr_columns(low, fields, allocation)?;
            collect_expr_columns(high, fields, allocation)?;
        }
        Expr::InList { expr, values, .. } => {
            collect_expr_columns(expr, fields, allocation)?;
            for value in values {
                collect_expr_columns(value, fields, allocation)?;
            }
        }
        Expr::Function(function) => {
            for argument in function.expressions() {
                collect_expr_columns(argument, fields, allocation)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn joined_fields(
    fields: &mut Vec<String>,
    allocation: &mut PlanAllocation<'_>,
) -> Result<String, CassieError> {
    if fields.is_empty() {
        return allocation.text("none");
    }
    fields.sort_unstable();
    fields.dedup();
    allocation.join(fields.iter().map(String::as_str), ",")
}

fn join_section(
    physical: &crate::planner::physical::PhysicalPlan,
    allocation: &mut PlanAllocation<'_>,
) -> Result<String, CassieError> {
    let join_keys = if physical.join.keys.is_empty() {
        allocation.text("none")?
    } else {
        allocation.join(physical.join.keys.iter().map(String::as_str), ",")?
    };
    let join_order = if physical.join.order.is_empty() {
        allocation.text("none")?
    } else {
        allocation.join(physical.join.order.iter().map(String::as_str), ">")?
    };
    let legality_barriers = if physical.join.legality_barriers.is_empty() {
        allocation.text("none")?
    } else {
        allocation.join(
            physical.join.legality_barriers.iter().map(String::as_str),
            ",",
        )?
    };
    let required_columns = if physical.join.properties.required_columns.is_empty() {
        allocation.text("none")?
    } else {
        allocation.join(
            physical
                .join
                .properties
                .required_columns
                .iter()
                .map(String::as_str),
            ",",
        )?
    };
    let required_ordering = if physical.join.properties.required_ordering.is_empty() {
        allocation.text("none")?
    } else {
        allocation.join(
            physical
                .join
                .properties
                .required_ordering
                .iter()
                .map(String::as_str),
            ",",
        )?
    };
    allocation.format(format_args!(
        "join_strategy={} join_enumeration={} join_order={} join_keys={} join_sort_required={} join_fallback_reason={} join_legality_barriers={} join_required_columns={} join_required_ordering={} join_parameterized={} join_rewindable={} join_bounded={} join_memory_bound={}",
        physical.join.strategy.as_deref().unwrap_or("none"),
        physical.join.enumeration,
        join_order,
        join_keys,
        physical.join.sort_required,
        physical.join.fallback_reason.as_deref().unwrap_or("none"),
        legality_barriers,
        required_columns,
        required_ordering,
        physical.join.properties.parameterized,
        physical.join.properties.rewindable,
        physical.join.properties.bounded,
        physical.join.properties.memory_bound
    ))
}

fn vectorized_join_section(
    cassie: &Cassie,
    physical: &crate::planner::physical::PhysicalPlan,

    allocation: &mut PlanAllocation<'_>,
) -> Result<String, CassieError> {
    let limits = cassie.runtime.limits();
    let vectorized = vectorized_join_status(physical, &limits);
    let operator_switch = operator_switch_status(physical, &limits);
    allocation.format(format_args!(
        "vectorized_join_candidate={} vectorized_join_enabled={} vectorized_join_batch_size={} vectorized_join_fallback_reason={} operator_switch_candidate={} operator_switch_enabled={} operator_switch_pair={} operator_switch_threshold={} operator_switch_reason={}",
        vectorized.candidate,
        vectorized.enabled,
        vectorized.batch_size,
        vectorized.fallback_reason,
        operator_switch.candidate,
        operator_switch.enabled,
        operator_switch.pair,
        operator_switch.threshold,
        operator_switch.reason
    ))
}

fn acceleration_section(
    cassie: &Cassie,
    physical: &crate::planner::physical::PhysicalPlan,

    allocation: &mut PlanAllocation<'_>,
) -> Result<String, CassieError> {
    let mixed = mixed_execution_diagnostics(physical, allocation)?;
    let rewrite = super::rollup_rewrite(cassie, physical, allocation)?;
    allocation.format(format_args!(
        "aggregate_parallel={} aggregate_acceleration={} rollup_rewrite={} mixed_execution={} mixed_stages={} exact_baseline={} projection_freshness={}",
        physical.aggregate.parallel_candidate,
        physical.aggregate.acceleration,
        rewrite,
        mixed.enabled,
        mixed.stages,
        mixed.exact_baseline,
        projection_freshness(cassie, physical)
    ))
}

fn cost_section(
    physical: &crate::planner::physical::PhysicalPlan,
    allocation: &mut PlanAllocation<'_>,
) -> Result<String, CassieError> {
    let estimates = &physical.estimates;
    let rejected_alternatives = if estimates.rejected_alternatives.is_empty() {
        allocation.text("none")?
    } else {
        allocation.join(
            estimates.rejected_alternatives.iter().map(String::as_str),
            ",",
        )?
    };
    allocation.format(format_args!(
        "cost_model=v{} selected_cost={} scan_cost={} index_cost={} cost_source={} rejected_alternatives={} estimates=scan:{} index:{} join:{} search:{} vector:{} aggregate:{}",
        estimates.cost_model_version,
        selected_cost(physical),
        estimates.scan_cost,
        estimates.index_cost,
        estimates.cost_source,
        rejected_alternatives,
        estimates.scan_rows,
        estimates.index_rows,
        estimates.join_rows,
        estimates.search_rows,
        estimates.vector_rows,
        estimates.aggregate_rows
    ))
}

struct VectorizedJoinStatus<'a> {
    candidate: bool,
    enabled: bool,
    batch_size: usize,
    fallback_reason: &'a str,
}

fn vectorized_join_status<'a>(
    physical: &'a crate::planner::physical::PhysicalPlan,
    limits: &crate::config::CassieRuntimeLimits,
) -> VectorizedJoinStatus<'a> {
    let candidate = physical.join.vectorized.candidate;
    let enabled = candidate && limits.vectorized_joins_enabled;
    let fallback_reason = if enabled {
        "none"
    } else if candidate {
        "disabled"
    } else {
        physical
            .join
            .vectorized
            .fallback_reason
            .as_deref()
            .unwrap_or("none")
    };
    VectorizedJoinStatus {
        candidate,
        enabled,
        batch_size: limits.vectorized_join_batch_size.max(1),
        fallback_reason,
    }
}

struct OperatorSwitchStatus {
    candidate: bool,
    enabled: bool,
    pair: &'static str,
    threshold: usize,
    reason: &'static str,
}

fn operator_switch_status(
    physical: &crate::planner::physical::PhysicalPlan,
    limits: &crate::config::CassieRuntimeLimits,
) -> OperatorSwitchStatus {
    let candidate = physical.join.vectorized.candidate;
    let enabled = candidate && limits.operator_switching_enabled.is_enabled();
    OperatorSwitchStatus {
        candidate,
        enabled,
        pair: if candidate {
            "vectorized_join_to_merge_join"
        } else {
            "none"
        },
        threshold: limits.operator_switch_join_row_threshold,
        reason: operator_switch_reason(candidate, enabled),
    }
}

fn operator_switch_reason(candidate: bool, enabled: bool) -> &'static str {
    if enabled {
        "armed"
    } else if candidate {
        "disabled"
    } else {
        "not_prevalidated"
    }
}

fn prefilter_description(
    cassie: &Cassie,
    physical: &crate::planner::physical::PhysicalPlan,
    allocation: &mut PlanAllocation<'_>,
) -> Result<String, CassieError> {
    let Some(filter) = physical.logical.filter.as_ref() else {
        return allocation.text("none");
    };
    if let Some(index) = physical.read.selected_index.as_deref() {
        return allocation.format(format_args!("index={index}"));
    }
    let schema = cassie
        .catalog
        .clone_schema_with_controls(&physical.collection, allocation.controls())?;
    match schema.0.as_ref() {
        Some(schema) if vector_prefilter_supported(filter, schema) => allocation.text("row-scan"),
        Some(schema) => allocation.format(format_args!(
            "fallback={}",
            vector_prefilter_fallback_reason(filter, schema)
        )),
        None => allocation.text("fallback=missing-schema"),
    }
}

fn time_series_description(
    cassie: &Cassie,
    physical: &crate::planner::physical::PhysicalPlan,
    allocation: &mut PlanAllocation<'_>,
) -> Result<String, CassieError> {
    let Some(name) = physical.read.selected_index.as_deref() else {
        return allocation.text("none");
    };
    cassie
        .catalog
        .with_index_metadata(&physical.collection, name, |index| {
            let Some(index) =
                index.filter(|index| index.kind == crate::catalog::IndexKind::TimeSeries)
            else {
                return allocation.text("none");
            };
            let bucket = index
                .options
                .get("bucket_width")
                .map_or("none", String::as_str);
            let partition = index
                .options
                .get("partition_by")
                .map_or("none", String::as_str);
            allocation.format(format_args!(
                "bucket_width:{bucket},partition_by:{partition},range_filter:{}",
                physical.logical.filter.is_some()
            ))
        })
}

fn time_series_storage_description(
    cassie: &Cassie,
    physical: &crate::planner::physical::PhysicalPlan,
    allocation: &mut PlanAllocation<'_>,
) -> Result<String, CassieError> {
    let Some(name) = physical.read.selected_index.as_deref() else {
        return allocation.text("none");
    };
    cassie
        .catalog
        .with_index_metadata(&physical.collection, name, |index| {
            let value =
                match index.filter(|index| index.kind == crate::catalog::IndexKind::TimeSeries) {
                    Some(index)
                        if time_series_bucket_width_supported(
                            index.options.get("bucket_width").map(String::as_str),
                        ) =>
                    {
                        "bucket-native-v1"
                    }
                    Some(_) => "row-backed-fallback",
                    None => "none",
                };
            allocation.text(value)
        })
}

fn time_series_bucket_width_supported(raw: Option<&str>) -> bool {
    let Some(raw) = raw else {
        return false;
    };
    let mut parts = raw.split_whitespace();
    let amount = parts.next().and_then(|value| value.parse::<u64>().ok());
    let unit = parts.next();
    amount.is_some_and(|value| value > 0)
        && parts.next().is_none()
        && unit.is_some_and(|unit| {
            ["minute", "minutes", "hour", "hours", "day", "days"]
                .iter()
                .any(|allowed| unit.eq_ignore_ascii_case(allowed))
        })
}

fn candidate_budget(
    cassie: &Cassie,
    physical: &crate::planner::physical::PhysicalPlan,
    allocation: &mut PlanAllocation<'_>,
) -> Result<String, CassieError> {
    let budget = physical.top_k.limit.map(|needed| {
        let limits = cassie.runtime.limits();
        let feedback = cassie
            .runtime
            .feedback_candidate_budget(&physical.collection)
            .unwrap_or_default();
        needed
            .max(limits.adaptive_candidate_min)
            .max(feedback)
            .min(limits.adaptive_candidate_max)
    });
    optional_number(budget, allocation)
}

fn optional_number(
    value: Option<usize>,
    allocation: &mut PlanAllocation<'_>,
) -> Result<String, CassieError> {
    match value {
        Some(value) => allocation.format(format_args!("{value}")),
        None => allocation.text("none"),
    }
}

#[derive(Clone, Copy)]
enum DiagnosticFlag {
    Enabled,
    Disabled,
}

impl DiagnosticFlag {
    const fn from_bool(value: bool) -> Self {
        if value {
            Self::Enabled
        } else {
            Self::Disabled
        }
    }

    const fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }
}

impl std::fmt::Display for DiagnosticFlag {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Enabled => "true",
            Self::Disabled => "false",
        })
    }
}

struct Phase03Diagnostics {
    column_native: DiagnosticFlag,
    hybrid_row_column: DiagnosticFlag,
    vectorized_aggregate: DiagnosticFlag,
    parallel_pipeline: DiagnosticFlag,
    analytical_projection: String,
}

fn phase03_diagnostics(
    cassie: &Cassie,
    physical: &crate::planner::physical::PhysicalPlan,
    allocation: &mut PlanAllocation<'_>,
) -> Result<Phase03Diagnostics, CassieError> {
    let column_native = physical.read.column_batch_index.is_some();
    let hybrid_row_column = column_native
        && (physical.logical.filter.is_some()
            || !physical.logical.order.is_empty()
            || !physical.logical.projection.is_empty());
    let parallel_pipeline = physical.aggregate.parallel_candidate
        || physical
            .operators
            .iter()
            .any(|operator| matches!(operator, crate::planner::physical::Operator::VectorSearch));
    let analytical_projection = cassie
        .catalog
        .with_analytical_projection_name(&physical.collection, |name| {
            allocation.text(name.unwrap_or("none"))
        })?;

    Ok(Phase03Diagnostics {
        column_native: DiagnosticFlag::from_bool(column_native),
        hybrid_row_column: DiagnosticFlag::from_bool(hybrid_row_column),
        vectorized_aggregate: DiagnosticFlag::from_bool(physical.aggregate.acceleration),
        parallel_pipeline: DiagnosticFlag::from_bool(parallel_pipeline),
        analytical_projection,
    })
}

struct MixedExecutionDiagnostics {
    enabled: bool,
    stages: String,
    exact_baseline: &'static str,
}

fn mixed_execution_diagnostics(
    physical: &crate::planner::physical::PhysicalPlan,
    allocation: &mut PlanAllocation<'_>,
) -> Result<MixedExecutionDiagnostics, CassieError> {
    let inputs = MixedExecutionInputs::from_plan(physical);
    let enabled = inputs.is_enabled();
    let stages = mixed_execution_stages(&inputs, allocation)?;

    Ok(MixedExecutionDiagnostics {
        enabled,
        stages,
        exact_baseline: if enabled {
            "source_row_exact_baseline"
        } else {
            "none"
        },
    })
}

struct MixedExecutionInputs {
    operators: MixedOperatorUse,
    clauses: MixedClauseUse,
}

struct MixedOperatorUse {
    fulltext: DiagnosticFlag,
    vector: DiagnosticFlag,
    aggregate: DiagnosticFlag,
}

struct MixedClauseUse {
    filter: DiagnosticFlag,
    order: DiagnosticFlag,
    offset: DiagnosticFlag,
    limit: DiagnosticFlag,
}

impl MixedExecutionInputs {
    fn from_plan(physical: &crate::planner::physical::PhysicalPlan) -> Self {
        Self {
            operators: MixedOperatorUse {
                fulltext: DiagnosticFlag::from_bool(has_operator(
                    physical,
                    &crate::planner::physical::Operator::FullTextSearch,
                )),
                vector: DiagnosticFlag::from_bool(has_operator(
                    physical,
                    &crate::planner::physical::Operator::VectorSearch,
                )),
                aggregate: DiagnosticFlag::from_bool(has_operator(
                    physical,
                    &crate::planner::physical::Operator::Aggregate,
                )),
            },
            clauses: MixedClauseUse {
                filter: DiagnosticFlag::from_bool(physical.logical.filter.is_some()),
                order: DiagnosticFlag::from_bool(!physical.logical.order.is_empty()),
                offset: DiagnosticFlag::from_bool(physical.logical.offset.is_some()),
                limit: DiagnosticFlag::from_bool(physical.logical.limit.is_some()),
            },
        }
    }

    fn is_enabled(&self) -> bool {
        let uses_fulltext = self.operators.fulltext.is_enabled();
        let uses_vector = self.operators.vector.is_enabled();
        let uses_aggregate = self.operators.aggregate.is_enabled();
        (uses_fulltext && uses_vector) || ((uses_fulltext || uses_vector) && uses_aggregate)
    }
}

fn has_operator(
    physical: &crate::planner::physical::PhysicalPlan,
    target: &crate::planner::physical::Operator,
) -> bool {
    physical.operators.contains(target)
}

fn mixed_execution_stages(
    inputs: &MixedExecutionInputs,
    allocation: &mut PlanAllocation<'_>,
) -> Result<String, CassieError> {
    let uses_search =
        inputs.operators.fulltext.is_enabled() || inputs.operators.vector.is_enabled();
    let stages = [
        (uses_search, "candidate_generation"),
        (inputs.clauses.filter.is_enabled(), "metadata_prefilter"),
        (uses_search, "exact_scoring"),
        (
            inputs.operators.aggregate.is_enabled(),
            "analytical_grouping",
        ),
        (inputs.clauses.order.is_enabled(), "ordering"),
        (inputs.clauses.offset.is_enabled(), "offset"),
        (inputs.clauses.limit.is_enabled(), "limit"),
    ];
    if !stages.iter().any(|(enabled, _)| *enabled) {
        return allocation.text("none");
    }
    allocation.join(
        stages
            .iter()
            .filter_map(|(enabled, value)| enabled.then_some(*value)),
        ">",
    )
}
