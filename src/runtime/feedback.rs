use super::*;
use crate::catalog::IndexMeta;
use crate::planner::logical::LogicalPlan;
use crate::sql::ast::{Expr, QuerySource, SelectItem, SortDirection};
use std::collections::BTreeSet;

pub(crate) const OPERATOR_FEEDBACK_CONFIDENCE_FLOOR_BPS: u16 = 600;
pub(crate) const OPERATOR_FEEDBACK_MIN_STABLE_SAMPLES: u64 = 3;
const OPERATOR_FEEDBACK_OUTLIER_RATIO: u64 = 8;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RuntimeFeedbackKey {
    pub schema_epoch: u64,
    pub database: Option<String>,
    pub collection: String,
    pub operator_family: String,
    pub relation_set: Vec<String>,
    pub predicate_shape_hash: u64,
    pub index_shape_hash: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RuntimeFeedbackRecord {
    pub executions: u64,
    pub rows_in_total: u64,
    pub rows_out_total: u64,
    pub elapsed_ms_total: u64,
    pub storage_reads_total: u64,
    pub storage_writes_total: u64,
    pub temp_writes_total: u64,
    pub candidate_count_total: u64,
    pub result_count_total: u64,
    pub errors_total: u64,
    pub last_error_class: Option<String>,
    pub stable_samples: u64,
    pub outlier_samples: u64,
    pub confidence_bps: u16,
    pub stable_rows_in_total: u64,
    pub stable_rows_out_total: u64,
    pub stable_elapsed_ms_total: u64,
    pub stable_storage_reads_total: u64,
    pub stable_storage_writes_total: u64,
    pub stable_temp_writes_total: u64,
    pub stable_candidate_count_total: u64,
    pub stable_result_count_total: u64,
    pub spill_samples: u64,
    pub memory_pressure_samples: u64,
    pub first_seen_ms: u64,
    pub last_seen_ms: u64,
}

impl RuntimeFeedbackRecord {
    pub fn stable_average_rows_in(&self) -> u64 {
        average(self.stable_rows_in_total, self.stable_samples)
    }

    pub fn stable_average_rows_out(&self) -> u64 {
        average(self.stable_rows_out_total, self.stable_samples)
    }

    pub fn stable_average_elapsed_ms(&self) -> u64 {
        average(self.stable_elapsed_ms_total, self.stable_samples)
    }

    pub fn stable_average_storage_reads(&self) -> u64 {
        average(self.stable_storage_reads_total, self.stable_samples)
    }

    pub fn stable_average_storage_writes(&self) -> u64 {
        average(self.stable_storage_writes_total, self.stable_samples)
    }

    pub fn stable_average_temp_writes(&self) -> u64 {
        average(self.stable_temp_writes_total, self.stable_samples)
    }

    pub fn stable_average_candidate_count(&self) -> u64 {
        average(self.stable_candidate_count_total, self.stable_samples)
    }

    pub fn stable_average_result_count(&self) -> u64 {
        average(self.stable_result_count_total, self.stable_samples)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RuntimeFeedbackObservation {
    pub rows_in: u64,
    pub rows_out: u64,
    pub elapsed_ms: u64,
    pub storage_reads: u64,
    pub storage_writes: u64,
    pub temp_writes: u64,
    pub candidate_count: u64,
    pub result_count: u64,
    pub error_class: Option<String>,
    pub spilled: bool,
    pub memory_pressure: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeFeedbackLookupState {
    Hit,
    Missing,
    Stale,
}

#[derive(Debug, Clone)]
pub(crate) struct RuntimeFeedbackLookup {
    pub state: RuntimeFeedbackLookupState,
    pub record: Option<RuntimeFeedbackRecord>,
    pub age_ms: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct OperatorFeedbackEstimate {
    pub state: &'static str,
    pub reason: &'static str,
    pub adjusted_cost: u64,
    pub confidence_bps: u16,
    pub age_ms: u64,
    pub samples: u64,
    pub outlier_samples: u64,
}

impl OperatorFeedbackEstimate {
    pub(crate) fn ignored(reason: &'static str, adjusted_cost: u64) -> Self {
        Self {
            state: "ignored",
            reason,
            adjusted_cost,
            confidence_bps: 0,
            age_ms: 0,
            samples: 0,
            outlier_samples: 0,
        }
    }

    pub(crate) fn used(
        adjusted_cost: u64,
        confidence_bps: u16,
        age_ms: u64,
        samples: u64,
        outlier_samples: u64,
    ) -> Self {
        Self {
            state: "used",
            reason: "applied",
            adjusted_cost,
            confidence_bps,
            age_ms,
            samples,
            outlier_samples,
        }
    }
}

pub(crate) fn normalized_feedback_key(
    database: Option<String>,
    schema_epoch: u64,
    collection: &str,
    operator_family: &str,
    plan: &LogicalPlan,
    index: Option<&IndexMeta>,
) -> RuntimeFeedbackKey {
    RuntimeFeedbackKey {
        schema_epoch,
        database,
        collection: collection.to_string(),
        operator_family: operator_family.to_string(),
        relation_set: relation_set_shape(&plan.source),
        predicate_shape_hash: predicate_shape_hash(plan),
        index_shape_hash: index.map(index_shape_hash),
    }
}

pub(crate) fn recompute_feedback_confidence(record: &mut RuntimeFeedbackRecord) {
    if record.stable_samples == 0 {
        record.confidence_bps = 0;
        return;
    }

    let sample_factor = (record.stable_samples.saturating_mul(250)).min(1_000) as u16;
    let consistency_factor = if record.executions == 0 {
        0
    } else {
        record
            .stable_samples
            .saturating_mul(1_000)
            .checked_div(record.executions)
            .unwrap_or(1_000) as u16
    };
    let error_penalty = if record.executions == 0 {
        1_000
    } else {
        1_000u64.saturating_sub(
            record
                .errors_total
                .saturating_mul(1_000)
                .checked_div(record.executions)
                .unwrap_or(1_000),
        )
    };
    record.confidence_bps = sample_factor
        .min(consistency_factor)
        .min(error_penalty.max(250) as u16);
}

pub(crate) fn observation_is_outlier(
    record: &RuntimeFeedbackRecord,
    observation: &RuntimeFeedbackObservation,
) -> bool {
    if record.stable_samples < OPERATOR_FEEDBACK_MIN_STABLE_SAMPLES.saturating_sub(1) {
        return false;
    }

    ratio_is_outlier(
        observation.elapsed_ms.max(1),
        record.stable_average_elapsed_ms().max(1),
    ) || ratio_is_outlier(
        observation.rows_in.max(observation.rows_out).max(1),
        record
            .stable_average_rows_in()
            .max(record.stable_average_rows_out())
            .max(1),
    ) || ratio_is_outlier(
        observation.storage_reads.max(1),
        record.stable_average_storage_reads().max(1),
    )
}

fn ratio_is_outlier(observed: u64, baseline: u64) -> bool {
    observed > baseline.saturating_mul(OPERATOR_FEEDBACK_OUTLIER_RATIO)
        || observed.saturating_mul(OPERATOR_FEEDBACK_OUTLIER_RATIO) < baseline
}

fn average(total: u64, samples: u64) -> u64 {
    if samples == 0 {
        0
    } else {
        total
            .saturating_add(samples.saturating_sub(1))
            .checked_div(samples)
            .unwrap_or(0)
    }
}

fn relation_set_shape(source: &QuerySource) -> Vec<String> {
    let mut relations = BTreeSet::new();
    collect_relation_names(source, &mut relations);
    relations.into_iter().collect()
}

fn collect_relation_names(source: &QuerySource, relations: &mut BTreeSet<String>) {
    match source {
        QuerySource::Collection(name) => {
            relations.insert(name.to_ascii_lowercase());
        }
        QuerySource::SingleRow => {
            relations.insert("__single_row__".to_string());
        }
        QuerySource::Cte(name) => {
            relations.insert(format!("cte:{}", name.to_ascii_lowercase()));
        }
        QuerySource::TableFunction { name, .. } => {
            relations.insert(format!("table_function:{}", name.to_ascii_lowercase()));
        }
        QuerySource::Subquery { alias, select, .. } => {
            relations.insert(format!("subquery:{}", alias.to_ascii_lowercase()));
            collect_relation_names(&select.source, relations);
        }
        QuerySource::Join { left, right, .. } => {
            collect_relation_names(left, relations);
            collect_relation_names(right, relations);
        }
    }
}

fn predicate_shape_hash(plan: &LogicalPlan) -> u64 {
    stable_fingerprint(&NormalizedPredicateShape::from(plan))
}

fn index_shape_hash(index: &IndexMeta) -> u64 {
    stable_fingerprint(&NormalizedIndexShape {
        kind: format!("{:?}", index.kind),
        fields: index.normalized_fields(),
        expressions: index.normalized_expressions(),
        include_fields: index.normalized_include_fields(),
        predicate_hash: index.predicate.as_ref().map(stable_fingerprint),
    })
}

#[derive(Serialize)]
struct NormalizedPredicateShape {
    distinct: bool,
    projection: Vec<NormalizedProjectionShape>,
    distinct_on: Vec<NormalizedExprShape>,
    filter: Option<NormalizedExprShape>,
    order: Vec<NormalizedOrderShape>,
    group_by: Vec<NormalizedExprShape>,
    having: Option<NormalizedExprShape>,
    has_limit: bool,
    has_offset: bool,
    has_set: bool,
}

impl From<&LogicalPlan> for NormalizedPredicateShape {
    fn from(plan: &LogicalPlan) -> Self {
        Self {
            distinct: plan.distinct,
            projection: plan
                .projection
                .iter()
                .map(NormalizedProjectionShape::from)
                .collect(),
            distinct_on: plan
                .distinct_on
                .iter()
                .map(NormalizedExprShape::from)
                .collect(),
            filter: plan.filter.as_ref().map(NormalizedExprShape::from),
            order: plan
                .order
                .iter()
                .map(|order| NormalizedOrderShape {
                    expr: NormalizedExprShape::from(&order.expr),
                    direction: match order.direction {
                        SortDirection::Asc => "asc",
                        SortDirection::Desc => "desc",
                    },
                    nulls: order.nulls.map(|nulls| format!("{nulls:?}")),
                })
                .collect(),
            group_by: plan
                .group_by
                .iter()
                .map(NormalizedExprShape::from)
                .collect(),
            having: plan.having.as_ref().map(NormalizedExprShape::from),
            has_limit: plan.limit.is_some(),
            has_offset: plan.offset.is_some(),
            has_set: plan.set.is_some(),
        }
    }
}

#[derive(Serialize)]
struct NormalizedOrderShape {
    expr: NormalizedExprShape,
    direction: &'static str,
    nulls: Option<String>,
}

#[derive(Serialize)]
struct NormalizedIndexShape {
    kind: String,
    fields: Vec<String>,
    expressions: Vec<String>,
    include_fields: Vec<String>,
    predicate_hash: Option<u64>,
}

#[derive(Serialize)]
enum NormalizedExprShape {
    Column(String),
    Value,
    Exists,
    Function {
        name: String,
        args: Vec<NormalizedExprShape>,
    },
    Binary {
        op: String,
        left: Box<NormalizedExprShape>,
        right: Box<NormalizedExprShape>,
    },
    InList {
        expr: Box<NormalizedExprShape>,
        values: Vec<NormalizedExprShape>,
        negated: bool,
    },
    Between {
        expr: Box<NormalizedExprShape>,
        low: Box<NormalizedExprShape>,
        high: Box<NormalizedExprShape>,
        negated: bool,
    },
    IsNull {
        expr: Box<NormalizedExprShape>,
        negated: bool,
    },
    Not(Box<NormalizedExprShape>),
    Cast {
        expr: Box<NormalizedExprShape>,
        data_type: String,
    },
}

impl From<&Expr> for NormalizedExprShape {
    fn from(expr: &Expr) -> Self {
        match expr {
            Expr::Column(name) => Self::Column(name.to_ascii_lowercase()),
            Expr::Param(_)
            | Expr::StringLiteral(_)
            | Expr::NumberLiteral(_)
            | Expr::BoolLiteral(_)
            | Expr::Null => Self::Value,
            Expr::Exists(_) => Self::Exists,
            Expr::Function(function) => Self::Function {
                name: function.name.to_ascii_lowercase(),
                args: function.args.iter().map(Self::from).collect(),
            },
            Expr::Binary { left, op, right } => Self::Binary {
                op: format!("{op:?}"),
                left: Box::new(Self::from(left.as_ref())),
                right: Box::new(Self::from(right.as_ref())),
            },
            Expr::InList {
                expr,
                values,
                negated,
            } => Self::InList {
                expr: Box::new(Self::from(expr.as_ref())),
                values: values.iter().map(Self::from).collect(),
                negated: *negated,
            },
            Expr::Between {
                expr,
                low,
                high,
                negated,
            } => Self::Between {
                expr: Box::new(Self::from(expr.as_ref())),
                low: Box::new(Self::from(low.as_ref())),
                high: Box::new(Self::from(high.as_ref())),
                negated: *negated,
            },
            Expr::IsNull { expr, negated } => Self::IsNull {
                expr: Box::new(Self::from(expr.as_ref())),
                negated: *negated,
            },
            Expr::Not { expr } => Self::Not(Box::new(Self::from(expr.as_ref()))),
            Expr::Cast { expr, data_type } => Self::Cast {
                expr: Box::new(Self::from(expr.as_ref())),
                data_type: format!("{data_type:?}"),
            },
        }
    }
}

#[derive(Serialize)]
enum NormalizedProjectionShape {
    Wildcard,
    Column(String),
    Function { name: String, arity: usize },
    Expr(NormalizedExprShape),
    WindowFunction { name: String, arity: usize },
}

impl From<&SelectItem> for NormalizedProjectionShape {
    fn from(item: &SelectItem) -> Self {
        match item {
            SelectItem::Wildcard => Self::Wildcard,
            SelectItem::Column { name, .. } => Self::Column(name.to_ascii_lowercase()),
            SelectItem::Function { function, .. } => Self::Function {
                name: function.name.to_ascii_lowercase(),
                arity: function.args.len(),
            },
            SelectItem::Expr { expr, .. } => Self::Expr(NormalizedExprShape::from(expr)),
            SelectItem::WindowFunction { function, .. } => Self::WindowFunction {
                name: function.name.to_ascii_lowercase(),
                arity: function.args.len(),
            },
        }
    }
}
