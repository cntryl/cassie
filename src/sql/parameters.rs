//! Parameter counts and current scalar inference; private implementation modules.

use crate::sql::{ast, ParsedStatement};
use crate::types::DataType;

mod admission;
mod count;
mod diagnostics;
mod fold;
mod function_inputs;
mod inference;
mod literal_input;
#[cfg(test)]
mod occurrence_tests;
mod occurrences;

use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use admission::{check_controls, checked_parameter_count, reserve_vector};
pub(crate) use admission::{AdmittedParameterTypes, ParameterAdmissionError};
pub(crate) use diagnostics::{ParameterSemanticDiagnostic, ScalarLiteralDiagnostic};
pub(crate) use occurrences::PrivateParameterTypeInput;

pub use count::parameter_count;
use inference::infer_parameter_type_oids_query;

const UNKNOWN_PARAMETER_TYPE_OID: i32 = 705;

struct ParameterInference<'a> {
    oids: Vec<i32>,
    context_demands: Vec<u8>,
    record_context_demands: bool,
    scope: Option<&'a crate::catalog::FunctionInputScope<'a>>,
    controls: Option<&'a QueryExecutionControls>,
    failure: Option<ParameterAdmissionError>,
}

impl ParameterInference<'_> {
    fn new(oids: Vec<i32>) -> Self {
        Self {
            context_demands: vec![0; oids.len()],
            oids,
            record_context_demands: true,
            scope: None,
            controls: None,
            failure: None,
        }
    }

    fn check(&mut self) -> bool {
        if self.failure.is_some() {
            return false;
        }
        if let Some(controls) = self.controls {
            if let Err(error) = check_controls(controls) {
                self.failure = Some(error);
                return false;
            }
        }
        true
    }

    fn finish_pass(&mut self) -> Result<(), ParameterAdmissionError> {
        self.check();
        self.failure.take().map_or(Ok(()), Err)
    }

    fn is_empty(&self) -> bool {
        self.oids.is_empty()
    }

    fn record_type_demand(&mut self, index: usize, data_type: &DataType) {
        if !self.record_context_demands {
            return;
        }
        let demand = match data_type {
            DataType::Boolean => 1,
            DataType::SmallInt | DataType::Int | DataType::BigInt | DataType::Float => 2,
            _ => return,
        };
        if let Some(contexts) = self.context_demands.get_mut(index) {
            *contexts |= demand;
        }
    }

    fn record_expression_demand(&mut self, expr: &ast::Expr, data_type: &DataType) {
        if let ast::Expr::Param(index) = expr {
            self.record_type_demand(*index, data_type);
        }
    }

    fn without_context_demands(&mut self, infer: impl FnOnce(&mut Self)) {
        let previous = std::mem::replace(&mut self.record_context_demands, false);
        infer(self);
        self.record_context_demands = previous;
    }
}

#[must_use]
pub fn parameter_type_oids(statement: &ParsedStatement, provided: &[i32]) -> Vec<i32> {
    let count = parameter_count(statement);
    let mut oids = provided.iter().copied().take(count).collect::<Vec<_>>();
    if oids.len() < count {
        oids.extend(std::iter::repeat_n(
            UNKNOWN_PARAMETER_TYPE_OID,
            count - oids.len(),
        ));
    }
    oids
}

#[must_use]
pub fn parameter_type_oids_with_catalog(
    statement: &ParsedStatement,
    provided: &[i32],
    catalog: &crate::catalog::Catalog,
) -> Vec<i32> {
    if uses_occurrence_fold(&statement.statement) {
        if let Some(occurrences) = public_fold(statement, provided, catalog) {
            let mut oids = occurrences.into_slots().values;
            for (oid, supplied) in oids.iter_mut().zip(provided) {
                if *supplied == 0 {
                    *oid = 0;
                }
            }
            return oids;
        }
        return parameter_type_oids(statement, provided);
    }
    infer_parameter_types_with_catalog(statement, provided, catalog).oids
}

/// Returns the original contextual targets for bound-string normalization.
///
/// UUID and temporal normalization need inferred coercion targets rather than
/// the declared input identity. This legacy normalization boundary keeps its
/// original inference and allocations; it must not start a fresh uncontrolled
/// occurrence fold after checked request admission.
#[must_use]
pub(crate) fn contextual_string_parameter_type_oids(
    statement: &ParsedStatement,
    catalog: &crate::catalog::Catalog,
) -> Vec<i32> {
    infer_parameter_types_with_catalog(statement, &[], catalog).oids
}

fn infer_parameter_types_with_catalog(
    statement: &ParsedStatement,
    provided: &[i32],
    catalog: &crate::catalog::Catalog,
) -> ParameterInference<'static> {
    let mut inference = ParameterInference::new(parameter_type_oids(statement, provided));
    infer_parameter_type_oids_query(&statement.statement, catalog, &mut inference);
    inference
}

/// Returns a planner error message when a client-declared parameter type
/// cannot be compared with, or assigned to, the `BOOLEAN` operand it meets.
///
/// Literals of other families are rejected against `BOOLEAN` operands by
/// operand-family validation; a declared parameter type is held to the same
/// rule, as PostgreSQL does, instead of being compared by truthiness.
/// Explicit CAST result types do not create implicit Boolean input demands.
#[must_use]
pub fn declared_parameter_type_conflict(
    statement: &ParsedStatement,
    provided: &[i32],
    catalog: &crate::catalog::Catalog,
) -> Option<String> {
    if uses_occurrence_fold(&statement.statement) {
        let occurrences = public_fold(statement, &[], catalog)?;
        return declared_conflict_for_demands(provided, &occurrences.demands.values);
    }
    let inference = infer_parameter_types_with_catalog(statement, &[], catalog);
    declared_conflict_for_demands(provided, &inference.context_demands)
}

fn declared_conflict_for_demands(provided: &[i32], demands: &[u8]) -> Option<String> {
    let boolean_oid = i32::try_from(DataType::Boolean.type_oid()).ok()?;
    if let Some(index) = demands.iter().position(|demand| demand & 3 == 3) {
        return Some(format!(
            "parameter ${} has incompatible boolean and numeric contexts",
            index + 1
        ));
    }
    provided.iter().zip(demands).find_map(|(declared, demand)| {
        if demand & 1 == 0
            || matches!(*declared, 0 | UNKNOWN_PARAMETER_TYPE_OID)
            || *declared == boolean_oid
        {
            return None;
        }
        let family = match *declared {
            20 | 21 | 23 | 700 | 701 | 1700 => "numeric",
            _ => "text",
        };
        Some(format!(
            "incompatible comparison operands: boolean and {family}"
        ))
    })
}

// Private Parse admission shares the original inference visitor and conflict
// projection. Public callers retain their existing Vec/Option contracts.
#[cfg(test)]
pub(crate) fn checked_parse_parameter_types(
    statement: &ParsedStatement,
    provided: &[i32],
    catalog: &crate::catalog::Catalog,
    controls: &QueryExecutionControls,
) -> Result<AdmittedParameterTypes, ParameterAdmissionError> {
    checked_parse_parameter_types_in_scope(statement, provided, catalog, controls, None)
}

#[cfg(test)]
pub(crate) fn checked_parse_parameter_types_in_scope(
    statement: &ParsedStatement,
    provided: &[i32],
    catalog: &crate::catalog::Catalog,
    controls: &QueryExecutionControls,
    scope: Option<&crate::catalog::FunctionInputScope<'_>>,
) -> Result<AdmittedParameterTypes, ParameterAdmissionError> {
    checked_parameter_types_in_scope(
        statement,
        PrivateParameterTypeInput::Wire(provided),
        catalog,
        controls,
        scope,
    )
}

pub(crate) fn checked_wire_parse_parameter_types_in_scope(
    statement: &ParsedStatement,
    provided: &[i32],
    catalog: &crate::catalog::Catalog,
    controls: &QueryExecutionControls,
    scope: Option<&crate::catalog::FunctionInputScope<'_>>,
) -> Result<AdmittedParameterTypes, ParameterAdmissionError> {
    checked_parameter_types_in_scope_with_wire_contract(
        statement,
        PrivateParameterTypeInput::Wire(provided),
        catalog,
        controls,
        scope,
        true,
    )
}

pub(crate) fn checked_parameter_types_in_scope(
    statement: &ParsedStatement,
    input: PrivateParameterTypeInput<'_>,
    catalog: &crate::catalog::Catalog,
    controls: &QueryExecutionControls,
    scope: Option<&crate::catalog::FunctionInputScope<'_>>,
) -> Result<AdmittedParameterTypes, ParameterAdmissionError> {
    checked_parameter_types_in_scope_with_wire_contract(
        statement, input, catalog, controls, scope, false,
    )
}

pub(crate) fn validate_wire_parse_parameter_slots(
    oids: &[i32],
    controls: &QueryExecutionControls,
) -> Result<(), ParameterAdmissionError> {
    for (slot, oid) in oids.iter().enumerate() {
        if slot % 256 == 0 {
            check_controls(controls)?;
        }
        if matches!(*oid, 0 | UNKNOWN_PARAMETER_TYPE_OID) {
            return Err(ParameterAdmissionError::Semantic(
                ParameterSemanticDiagnostic::Unresolved { slot },
            ));
        }
    }
    check_controls(controls)
}

fn checked_parameter_types_in_scope_with_wire_contract(
    statement: &ParsedStatement,
    input: PrivateParameterTypeInput<'_>,
    catalog: &crate::catalog::Catalog,
    controls: &QueryExecutionControls,
    scope: Option<&crate::catalog::FunctionInputScope<'_>>,
    enforce_wire_parse_contract: bool,
) -> Result<AdmittedParameterTypes, ParameterAdmissionError> {
    let count = if enforce_wire_parse_contract {
        checked_effective_parameter_count(statement, input, controls)?
    } else {
        checked_parameter_count(statement, controls)?
    };
    if !uses_occurrence_fold(&statement.statement) {
        return checked_legacy_parameter_types(statement, input, count, catalog, controls, scope);
    }
    let result =
        fold::Fold::new(count, input, catalog, controls, scope, true)?.run(&statement.statement)?;
    if let Some(message) =
        declared_conflict_for_demands(occurrences::input_oids(input), &result.demands.values)
    {
        return Err(ParameterAdmissionError::LegacyConflict(message));
    }
    let slots = result.into_slots();
    check_controls(controls)?;
    Ok(AdmittedParameterTypes::new(
        slots.values,
        slots.reservation,
        controls,
    ))
}

fn checked_legacy_parameter_types(
    statement: &ParsedStatement,
    input: PrivateParameterTypeInput<'_>,
    count: usize,
    catalog: &crate::catalog::Catalog,
    controls: &QueryExecutionControls,
    scope: Option<&crate::catalog::FunctionInputScope<'_>>,
) -> Result<AdmittedParameterTypes, ParameterAdmissionError> {
    let provided = occurrences::input_oids(input);
    let mut normalized = reserve_vector::<i32>(controls, provided.len())?;
    for (index, oid) in provided.iter().enumerate() {
        if index % 256 == 0 {
            check_controls(controls)?;
        }
        normalized.values.push(if *oid == 0 {
            UNKNOWN_PARAMETER_TYPE_OID
        } else {
            *oid
        });
    }
    {
        // The conflict pass deliberately starts UNKNOWN, independently of
        // supplied OIDs. Both its vectors drop before the second pass begins.
        let mut first = AccountedInference::new(count, &[], controls, scope)?;
        check_controls(controls)?;
        infer_parameter_type_oids_query(&statement.statement, catalog, &mut first.value);
        first.value.finish_pass()?;
        if let Some(message) =
            declared_conflict_for_demands(&normalized.values, &first.value.context_demands)
        {
            return Err(ParameterAdmissionError::LegacyConflict(message));
        }
    }
    let mut actual = AccountedInference::new(count, &normalized.values, controls, scope)?;
    check_controls(controls)?;
    infer_parameter_type_oids_query(&statement.statement, catalog, &mut actual.value);
    actual.value.finish_pass()?;
    let AccountedInference {
        value,
        oid_memory,
        demand_memory,
    } = actual;
    let ParameterInference {
        oids,
        context_demands,
        record_context_demands: _,
        scope: _,
        controls: _,
        failure: _,
    } = value;
    drop(context_demands);
    drop(demand_memory);
    Ok(AdmittedParameterTypes::new(oids, oid_memory, controls))
}

fn checked_effective_parameter_count(
    statement: &ParsedStatement,
    input: PrivateParameterTypeInput<'_>,
    controls: &QueryExecutionControls,
) -> Result<usize, ParameterAdmissionError> {
    let sql_count = checked_parameter_count(statement, controls)?;
    let supplied_count = match input {
        PrivateParameterTypeInput::Wire(oids) => oids.len(),
        PrivateParameterTypeInput::Embedded { .. } => 0,
    };
    let count = sql_count.max(supplied_count);
    if count > admission::MAX_PARAMETER_COUNT {
        return Err(ParameterAdmissionError::InvalidParam(format!(
            "parameter index exceeds PostgreSQL limit {}",
            admission::MAX_PARAMETER_COUNT
        )));
    }
    Ok(count)
}

fn uses_occurrence_fold(mut query: &crate::sql::QueryStatement) -> bool {
    while let crate::sql::QueryStatement::Explain(explain) = query {
        query = &explain.statement.statement;
    }
    matches!(query, crate::sql::QueryStatement::Select(_))
}

fn public_fold<'a>(
    statement: &'a ParsedStatement,
    provided: &'a [i32],
    catalog: &crate::catalog::Catalog,
) -> Option<occurrences::Occurrences<'a>> {
    // The compatibility facade has no error or resource-control parameter.
    // Runtime callers always use checked_parameter_types_in_scope instead.
    let controls = QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits {
            query_memory_budget_bytes: usize::MAX,
            query_timeout_ms: 0,
            ..crate::config::CassieRuntimeLimits::default()
        },
        std::time::Instant::now(),
    );
    let count = checked_parameter_count(statement, &controls).ok()?;
    fold::Fold::new(
        count,
        PrivateParameterTypeInput::Wire(provided),
        catalog,
        &controls,
        None,
        false,
    )
    .ok()?
    .run(&statement.statement)
    .ok()
}

struct AccountedInference<'a> {
    value: ParameterInference<'a>,
    oid_memory: QueryMemoryReservation,
    demand_memory: QueryMemoryReservation,
}

impl<'a> AccountedInference<'a> {
    fn new(
        count: usize,
        provided: &[i32],
        controls: &'a QueryExecutionControls,
        scope: Option<&'a crate::catalog::FunctionInputScope<'a>>,
    ) -> Result<Self, ParameterAdmissionError> {
        let mut oids = reserve_vector::<i32>(controls, count)?;
        let mut demands = reserve_vector::<u8>(controls, count)?;
        for index in 0..count {
            if index % 256 == 0 {
                check_controls(controls)?;
            }
            oids.values.push(
                provided
                    .get(index)
                    .copied()
                    .unwrap_or(UNKNOWN_PARAMETER_TYPE_OID),
            );
            demands.values.push(0);
        }
        Ok(Self {
            value: ParameterInference {
                oids: oids.values,
                context_demands: demands.values,
                record_context_demands: true,
                scope,
                controls: Some(controls),
                failure: None,
            },
            oid_memory: oids.reservation,
            demand_memory: demands.reservation,
        })
    }
}
