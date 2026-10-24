//! Admitted, ordinal occurrence identities and expression output summaries.

use crate::runtime::QueryExecutionControls;
use crate::types::Value;

use super::admission::{check_controls, reserve_vector, ReservedVector};
use super::diagnostics::{BooleanConstruct, ParameterSemanticDiagnostic};
use super::{ParameterAdmissionError, UNKNOWN_PARAMETER_TYPE_OID};

// Low bits retain the original Boolean/numeric demand projection. This bit
// records only a slot inferred through a context outside occurrence parity.
const LEGACY_INFERRED: u8 = 4;

#[derive(Clone, Copy)]
pub(crate) enum PrivateParameterTypeInput<'a> {
    Wire(&'a [i32]),
    Embedded {
        effective: &'a [i32],
        values: &'a [Value],
    },
}

pub(super) fn input_oids(input: PrivateParameterTypeInput<'_>) -> &[i32] {
    match input {
        PrivateParameterTypeInput::Wire(oids) => oids,
        PrivateParameterTypeInput::Embedded { effective, .. } => effective,
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Summary {
    pub(super) oid: i32,
    pub(super) occurrence: Option<usize>,
    pub(super) qualified: bool,
    pub(super) literal: bool,
}

impl Summary {
    pub(super) const fn known(oid: i32) -> Self {
        Self {
            oid,
            occurrence: None,
            qualified: primitive_oid(oid),
            literal: false,
        }
    }

    pub(super) const fn unqualified() -> Self {
        Self {
            oid: UNKNOWN_PARAMETER_TYPE_OID,
            occurrence: None,
            qualified: false,
            literal: false,
        }
    }

    pub(super) const fn literal() -> Self {
        Self {
            oid: UNKNOWN_PARAMETER_TYPE_OID,
            occurrence: None,
            qualified: true,
            literal: true,
        }
    }
}

pub(super) const fn primitive_oid(oid: i32) -> bool {
    matches!(
        oid,
        16 | 17
            | 20
            | 21
            | 23
            | 25
            | 26
            | 700
            | 701
            | 1042
            | 1043
            | 1082
            | 1083
            | 1114
            | 1700
            | 2950
    )
}

#[derive(Clone, Copy)]
struct Occurrence {
    slot: usize,
    creation_oid: i32,
    transformed_oid: i32,
    contextual_null: bool,
    qualified: bool,
}

pub(super) struct Occurrences<'a> {
    pub(super) slots: ReservedVector<i32>,
    pub(super) demands: ReservedVector<u8>,
    ledger: ReservedVector<Occurrence>,
    input: PrivateParameterTypeInput<'a>,
    pub(super) diagnostic: Option<ParameterSemanticDiagnostic>,
    stop_on_error: bool,
    pub(super) binding_required: bool,
}

impl<'a> Occurrences<'a> {
    pub(super) fn stops_on_error(&self) -> bool {
        self.stop_on_error
    }
    pub(super) fn into_slots(self) -> ReservedVector<i32> {
        self.slots
    }
    pub(super) fn len(&self) -> usize {
        self.ledger.values.len()
    }

    pub(super) fn invalid_scalar_literal(
        &self,
        input: &str,
        diagnostic: super::diagnostics::ScalarLiteralDiagnostic,
        controls: &QueryExecutionControls,
    ) -> Result<(), ParameterAdmissionError> {
        if self.stop_on_error {
            return Err(super::literal_input::admit(input, diagnostic, controls)?);
        }
        Ok(())
    }

    pub(super) fn invalid_boolean_literal(
        &self,
        input: &str,
        controls: &QueryExecutionControls,
    ) -> Result<(), ParameterAdmissionError> {
        if self.stop_on_error
            && matches!(self.input, PrivateParameterTypeInput::Wire(_))
            && crate::types::boolean::parse_text(input).is_none()
        {
            return Err(super::admission::admit_invalid_boolean_literal(
                input, controls,
            )?);
        }
        Ok(())
    }

    pub(super) fn boolean_aggregate_literal(
        &self,
        input: &str,
        controls: &QueryExecutionControls,
    ) -> Result<(), ParameterAdmissionError> {
        if self.stop_on_error && crate::types::boolean::parse_text(input).is_none() {
            return Err(super::admission::admit_invalid_boolean_literal(
                input, controls,
            )?);
        }
        Ok(())
    }

    pub(super) fn boolean_aggregate_input(
        &mut self,
        function: &'static str,
        summary: Summary,
    ) -> Result<(), ParameterAdmissionError> {
        if !matches!(summary.oid, 0 | 16 | UNKNOWN_PARAMETER_TYPE_OID) {
            self.undefined_function(function, 1, Some(summary.oid))?;
        }
        Ok(())
    }

    pub(super) fn unresolved_parameter(
        &mut self,
        slot: usize,
    ) -> Result<(), ParameterAdmissionError> {
        self.report(ParameterSemanticDiagnostic::Unresolved { slot })
    }

    pub(super) fn pending_datum_input(&mut self) -> Result<(), ParameterAdmissionError> {
        self.report(ParameterSemanticDiagnostic::PendingDatumInput)
    }

    pub(super) fn pending_name_input(&mut self) -> Result<(), ParameterAdmissionError> {
        self.report(ParameterSemanticDiagnostic::PendingNameInput)
    }

    pub(super) fn pending_type_function_input(&mut self) -> Result<(), ParameterAdmissionError> {
        self.report(ParameterSemanticDiagnostic::PendingTypeFunctionInput)
    }

    pub(super) fn pending_window_type_coercion(&mut self) -> Result<(), ParameterAdmissionError> {
        self.report(ParameterSemanticDiagnostic::PendingWindowTypeCoercion)
    }

    pub(super) fn ambiguous_function(
        &mut self,
        function: &'static str,
    ) -> Result<(), ParameterAdmissionError> {
        self.report(ParameterSemanticDiagnostic::AmbiguousFunction { function })
    }

    pub(super) fn undefined_function(
        &mut self,
        function: &'static str,
        argument_count: usize,
        actual: Option<i32>,
    ) -> Result<(), ParameterAdmissionError> {
        self.report(ParameterSemanticDiagnostic::UndefinedFunction {
            function,
            argument_count,
            actual,
        })
    }

    pub(super) fn values_width(&mut self) -> Result<(), ParameterAdmissionError> {
        self.report(ParameterSemanticDiagnostic::ValuesWidth)
    }

    pub(super) fn values_types(
        &mut self,
        left: i32,
        right: i32,
    ) -> Result<(), ParameterAdmissionError> {
        self.report(ParameterSemanticDiagnostic::ValuesTypes { left, right })
    }

    pub(super) fn invalid_distinct_order(&mut self) -> Result<(), ParameterAdmissionError> {
        self.report(ParameterSemanticDiagnostic::DistinctAggregateOrder)
    }

    pub(super) fn missing_equality(&mut self, actual: i32) -> Result<(), ParameterAdmissionError> {
        self.report(ParameterSemanticDiagnostic::MissingEquality { actual })
    }

    pub(super) fn unqualify_since(
        &mut self,
        start: usize,
        controls: &QueryExecutionControls,
    ) -> Result<(), ParameterAdmissionError> {
        for occurrence in &mut self.ledger.values[start..] {
            check_controls(controls)?;
            occurrence.qualified = false;
            if occurrence.creation_oid == UNKNOWN_PARAMETER_TYPE_OID
                && occurrence.transformed_oid != UNKNOWN_PARAMETER_TYPE_OID
            {
                self.demands.values[occurrence.slot] |= LEGACY_INFERRED;
            }
        }
        Ok(())
    }
    pub(super) fn new(
        count: usize,
        input: PrivateParameterTypeInput<'a>,
        controls: &QueryExecutionControls,
        stop_on_error: bool,
    ) -> Result<Self, ParameterAdmissionError> {
        // Admit the entire supplied prefix even when the SQL has no parameters.
        let supplied = input_oids(input);
        let mut normalized = reserve_vector::<i32>(controls, supplied.len())?;
        for oid in supplied {
            check_controls(controls)?;
            normalized.values.push(if *oid == 0 {
                UNKNOWN_PARAMETER_TYPE_OID
            } else {
                *oid
            });
        }
        let mut slots = reserve_vector::<i32>(controls, count)?;
        let mut demands = reserve_vector::<u8>(controls, count)?;
        for index in 0..count {
            check_controls(controls)?;
            slots.values.push(
                normalized
                    .values
                    .get(index)
                    .copied()
                    .unwrap_or(UNKNOWN_PARAMETER_TYPE_OID),
            );
            demands.values.push(0);
        }
        drop(normalized);
        Ok(Self {
            slots,
            demands,
            ledger: reserve_vector(controls, 0)?,
            input,
            diagnostic: None,
            stop_on_error,
            binding_required: false,
        })
    }

    pub(super) fn parameter(
        &mut self,
        slot: usize,
        controls: &QueryExecutionControls,
        qualified: bool,
    ) -> Result<Summary, ParameterAdmissionError> {
        let oid = self
            .slots
            .values
            .get(slot)
            .copied()
            .unwrap_or(UNKNOWN_PARAMETER_TYPE_OID);
        let qualified = qualified && (oid == UNKNOWN_PARAMETER_TYPE_OID || primitive_oid(oid));
        let contextual_null = match self.input {
            PrivateParameterTypeInput::Embedded { effective, values } => {
                matches!(values.get(slot), Some(Value::Null))
                    && effective
                        .get(slot)
                        .is_none_or(|oid| matches!(*oid, 0 | UNKNOWN_PARAMETER_TYPE_OID))
            }
            PrivateParameterTypeInput::Wire(_) => false,
        };
        let ordinal = self.ledger.values.len();
        self.ledger.try_push(
            Occurrence {
                slot,
                creation_oid: oid,
                transformed_oid: oid,
                contextual_null,
                qualified,
            },
            controls,
        )?;
        Ok(Summary {
            oid,
            occurrence: Some(ordinal),
            qualified,
            literal: contextual_null,
        })
    }

    pub(super) fn coerce(
        &mut self,
        summary: &mut Summary,
        target: i32,
        record_demand: bool,
    ) -> Result<(), ParameterAdmissionError> {
        let Some(ordinal) = summary.occurrence else {
            return Ok(());
        };
        let occurrence = &mut self.ledger.values[ordinal];
        if record_demand {
            let demand = match target {
                16 => 1,
                20 | 21 | 23 | 700 | 701 | 1700 => 2,
                _ => 0,
            };
            self.demands.values[occurrence.slot] |= demand;
        }
        if occurrence.transformed_oid == UNKNOWN_PARAMETER_TYPE_OID {
            let slot = &mut self.slots.values[occurrence.slot];
            if *slot != UNKNOWN_PARAMETER_TYPE_OID && *slot != target && !occurrence.contextual_null
            {
                if !occurrence.qualified
                    || self.demands.values[occurrence.slot] & LEGACY_INFERRED != 0
                {
                    occurrence.transformed_oid = *slot;
                    summary.oid = *slot;
                    return Ok(());
                }
                let diagnostic = ParameterSemanticDiagnostic::Inconsistent {
                    slot: occurrence.slot,
                    previous: *slot,
                    requested: target,
                };
                return self.report(diagnostic);
            }
            if *slot == UNKNOWN_PARAMETER_TYPE_OID {
                if !occurrence.qualified {
                    self.demands.values[occurrence.slot] |= LEGACY_INFERRED;
                }
                *slot = target;
            }
            occurrence.transformed_oid = target;
            summary.oid = target;
        }
        Ok(())
    }

    pub(super) fn embedded_int4_carrier(&self, summary: Summary) -> bool {
        let PrivateParameterTypeInput::Embedded { values, .. } = self.input else {
            return false;
        };
        summary
            .occurrence
            .and_then(|ordinal| self.ledger.values.get(ordinal))
            .and_then(|occurrence| values.get(occurrence.slot))
            .is_some_and(
                |value| matches!(value, Value::Int64(value) if i32::try_from(*value).is_ok()),
            )
    }

    pub(super) fn boolean(
        &mut self,
        summary: &mut Summary,
        construct: BooleanConstruct,
        diagnose: bool,
    ) -> Result<(), ParameterAdmissionError> {
        let selected_for_inference = matches!(
            construct,
            BooleanConstruct::Where
                | BooleanConstruct::Not
                | BooleanConstruct::And
                | BooleanConstruct::Or
                | BooleanConstruct::CaseWhen
                | BooleanConstruct::Truth(..)
        );
        let selected_for_diagnostics = selected_for_inference
            || matches!(
                construct,
                BooleanConstruct::Having | BooleanConstruct::Join | BooleanConstruct::Filter
            );
        if !selected_for_inference {
            if let Some(ordinal) = summary.occurrence {
                let occurrence = self.ledger.values[ordinal];
                if occurrence.transformed_oid == UNKNOWN_PARAMETER_TYPE_OID
                    && self.slots.values[occurrence.slot] == UNKNOWN_PARAMETER_TYPE_OID
                {
                    self.demands.values[occurrence.slot] |= LEGACY_INFERRED;
                }
            }
        }
        self.coerce(summary, 16, true)?;
        if diagnose
            && selected_for_diagnostics
            && summary.qualified
            && !self.has_legacy_inference(summary)
            && !summary.literal
            && !matches!(summary.oid, 16 | UNKNOWN_PARAMETER_TYPE_OID)
        {
            self.report(ParameterSemanticDiagnostic::BooleanOperand {
                construct,
                actual: summary.oid,
            })?;
        }
        Ok(())
    }

    pub(super) fn finalize_target(
        &mut self,
        summary: &mut Summary,
    ) -> Result<(), ParameterAdmissionError> {
        if summary.qualified
            && !self.has_legacy_inference(summary)
            && !summary.literal
            && summary.oid == UNKNOWN_PARAMETER_TYPE_OID
        {
            self.coerce(summary, 25, false)?;
        }
        Ok(())
    }

    pub(super) fn finish(
        &mut self,
        controls: &QueryExecutionControls,
    ) -> Result<(), ParameterAdmissionError> {
        for index in 0..self.ledger.values.len() {
            check_controls(controls)?;
            let occurrence = self.ledger.values[index];
            if occurrence.qualified
                && self.demands.values[occurrence.slot] & LEGACY_INFERRED == 0
                && !occurrence.contextual_null
                && occurrence.transformed_oid != self.slots.values[occurrence.slot]
            {
                self.report(ParameterSemanticDiagnostic::Indeterminate {
                    slot: occurrence.slot,
                })?;
            }
            debug_assert!(occurrence.creation_oid != 0);
        }
        Ok(())
    }

    fn has_legacy_inference(&self, summary: &Summary) -> bool {
        summary.occurrence.is_some_and(|ordinal| {
            self.demands.values[self.ledger.values[ordinal].slot] & LEGACY_INFERRED != 0
        })
    }

    fn report(
        &mut self,
        diagnostic: ParameterSemanticDiagnostic,
    ) -> Result<(), ParameterAdmissionError> {
        self.diagnostic.get_or_insert(diagnostic);
        if self.stop_on_error {
            Err(ParameterAdmissionError::Semantic(diagnostic))
        } else {
            Ok(())
        }
    }
}
