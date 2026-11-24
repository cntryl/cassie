//! Parameter admission of the `width_bucket` overloads.

use crate::sql::{ast, functions};

use super::{check_controls, Fold, Frame, ParameterAdmissionError, Summary};
use crate::types::DataType;

impl Fold<'_, '_> {
    pub(super) fn width_bucket_input_signature(
        &mut self,
        function: &ast::FunctionCall,
        metadata: Option<&functions::ScalarFunction>,
        frame: &Frame<'_>,
    ) -> Result<Option<Summary>, ParameterAdmissionError> {
        let is_width_bucket = metadata.is_some_and(|entry| {
            entry.id == functions::FunctionId::Math(functions::MathFunction::WidthBucket)
        }) && function.args.len() == 4;
        if !is_width_bucket {
            return Ok(None);
        }
        if let Some(summary) = self.width_bucket_literal_family(function, frame)? {
            return Ok(Some(summary));
        }
        if let Some(summary) = self.width_bucket_unknown_parameters(function, frame)? {
            return Ok(Some(summary));
        }
        self.validate_width_bucket_literal_count(function, frame)?;
        if let Some(summary) = self.width_bucket_parameter_count(function, frame)? {
            return Ok(Some(summary));
        }
        self.width_bucket_mixed_targets(function, frame)
    }

    pub(super) fn width_bucket_literal_family(
        &mut self,
        function: &ast::FunctionCall,
        frame: &Frame<'_>,
    ) -> Result<Option<Summary>, ParameterAdmissionError> {
        let summaries = &self.summaries.values[frame.start..frame.start + 4];
        if let Some((slot, previous, requested)) =
            mixed_bucket_count_conflict(&function.args, summaries)
        {
            self.occurrences
                .inconsistent_parameter_type(slot, previous, requested)?;
            return Ok(Some(Summary::known(23)));
        }
        let fresh = std::array::from_fn(|index| {
            let summary = &summaries[index];
            summary.occurrence.is_some() && !self.occurrences.has_legacy_inference(summary)
        });
        if !unknown_bucket_literal_operands(&function.args, summaries, fresh) {
            return Ok(None);
        }
        if let ast::Expr::StringLiteral(input) = &function.args[3] {
            check_controls(self.controls)?;
            self.validate_type_function_literal(input, &DataType::Int)?;
        }
        for argument in &mut self.summaries.values[frame.start..frame.start + 3] {
            check_controls(self.controls)?;
            self.occurrences.coerce(argument, 701, true)?;
        }
        Ok(Some(Summary::known(23)))
    }

    pub(super) fn width_bucket_unknown_parameters(
        &mut self,
        function: &ast::FunctionCall,
        frame: &Frame<'_>,
    ) -> Result<Option<Summary>, ParameterAdmissionError> {
        let summaries = &self.summaries.values[frame.start..frame.start + 4];
        let shared_slot_is_fresh = summaries.iter().all(|summary| {
            summary.occurrence.is_some() && !self.occurrences.has_legacy_inference(summary)
        });
        if !all_unknown_bucket_parameters(&function.args, summaries, shared_slot_is_fresh) {
            return Ok(None);
        }
        for (argument, target) in self.summaries.values[frame.start..frame.start + 4]
            .iter_mut()
            .zip([701, 701, 701, 23])
        {
            check_controls(self.controls)?;
            self.occurrences.coerce(argument, target, true)?;
        }
        Ok(Some(Summary::known(23)))
    }

    pub(super) fn validate_width_bucket_literal_count(
        &mut self,
        function: &ast::FunctionCall,
        frame: &Frame<'_>,
    ) -> Result<(), ParameterAdmissionError> {
        let arguments = &self.summaries.values[frame.start..frame.start + 4];
        let first = arguments[0].oid;
        if matches!(first, 701 | 1700)
            && arguments[..3]
                .iter()
                .all(|argument| argument.qualified && argument.oid == first)
        {
            if let ast::Expr::StringLiteral(input) = &function.args[3] {
                check_controls(self.controls)?;
                self.validate_type_function_literal(input, &DataType::Int)?;
            }
        }
        Ok(())
    }

    pub(super) fn width_bucket_parameter_count(
        &mut self,
        function: &ast::FunctionCall,
        frame: &Frame<'_>,
    ) -> Result<Option<Summary>, ParameterAdmissionError> {
        if !matches!(function.args[3], ast::Expr::Param(_)) {
            return Ok(None);
        }
        let arguments = &self.summaries.values[frame.start..frame.start + 4];
        let first = arguments[0].oid;
        if !matches!(first, 701 | 1700)
            || !arguments[..3]
                .iter()
                .all(|argument| argument.qualified && argument.oid == first)
            || !matches!(arguments[3].oid, 0 | 705)
        {
            return Ok(None);
        }
        check_controls(self.controls)?;
        self.occurrences
            .coerce(&mut self.summaries.values[frame.start + 3], 23, true)?;
        Ok(Some(Summary::known(23)))
    }

    pub(super) fn width_bucket_mixed_targets(
        &mut self,
        function: &ast::FunctionCall,
        frame: &Frame<'_>,
    ) -> Result<Option<Summary>, ParameterAdmissionError> {
        let summaries = &self.summaries.values[frame.start..frame.start + 4];
        let fresh = std::array::from_fn(|index| {
            let summary = &summaries[index];
            summary.occurrence.is_some() && !self.occurrences.has_legacy_inference(summary)
        });
        let Some(targets) = mixed_bucket_targets(&function.args, summaries, fresh) else {
            return Ok(None);
        };
        for (argument, target) in self.summaries.values[frame.start..frame.start + 4]
            .iter_mut()
            .zip(targets)
        {
            check_controls(self.controls)?;
            if matches!(argument.oid, 0 | 705) {
                self.occurrences.coerce(argument, target, true)?;
            }
        }
        Ok(Some(Summary::known(23)))
    }
}

pub(super) fn all_unknown_bucket_parameters(
    arguments: &[ast::Expr],
    summaries: &[Summary],
    shared_slot_is_fresh: bool,
) -> bool {
    let [ast::Expr::Param(first), ast::Expr::Param(second), ast::Expr::Param(third), ast::Expr::Param(count)] =
        arguments
    else {
        return false;
    };
    (![first, second, third].contains(&count)
        || shared_slot_is_fresh && summaries.iter().all(|summary| !summary.literal))
        && summaries.len() == 4
        && summaries
            .iter()
            .all(|summary| summary.qualified && matches!(summary.oid, 0 | 705))
}

pub(super) fn mixed_bucket_targets(
    arguments: &[ast::Expr],
    summaries: &[Summary],
    fresh: [bool; 4],
) -> Option<[i32; 4]> {
    if arguments.len() != 4 || summaries.len() != 4 {
        return None;
    }
    let mut anchor = None;
    let mut slots = [None; 3];
    let mut unknowns = 0;
    for index in 0..3 {
        let summary = summaries[index];
        if !summary.qualified {
            return None;
        }
        if let ast::Expr::Param(slot) = &arguments[index] {
            if !matches!(summary.oid, 0 | 705) || summary.literal || !fresh[index] {
                return None;
            }
            slots[index] = Some(*slot);
            unknowns += 1;
        } else if matches!(summary.oid, 701 | 1700) {
            if anchor.is_some_and(|known| known != summary.oid) {
                return None;
            }
            anchor = Some(summary.oid);
        } else {
            return None;
        }
    }
    let anchor = match anchor {
        Some(anchor) => anchor,
        None if matches!(summaries[3].oid, 21 | 23) => 701,
        None => return None,
    };
    if unknowns == 0 || !summaries[3].qualified {
        return None;
    }
    if matches!(summaries[3].oid, 0 | 705) {
        let ast::Expr::Param(slot) = &arguments[3] else {
            return None;
        };
        if summaries[3].literal || !fresh[3] || slots.contains(&Some(*slot)) {
            return None;
        }
    } else if !matches!(summaries[3].oid, 21 | 23) {
        return None;
    }
    Some([anchor, anchor, anchor, 23])
}

fn mixed_bucket_count_conflict(
    arguments: &[ast::Expr],
    summaries: &[Summary],
) -> Option<(usize, i32, i32)> {
    if arguments.len() != 4 || summaries.len() != 4 || !matches!(summaries[3].oid, 21 | 23) {
        return None;
    }
    let mut anchor = None;
    for summary in &summaries[..3] {
        if matches!(summary.oid, 701 | 1700) {
            if anchor.is_some_and(|known| known != summary.oid) {
                return None;
            }
            anchor = Some(summary.oid);
        }
    }
    let requested = anchor?;
    for index in 0..3 {
        let ast::Expr::Param(slot) = &arguments[index] else {
            continue;
        };
        if !summaries[index].qualified
            || !matches!(summaries[index].oid, 0 | 705)
            || !contains_integer_cast_of_parameter(&arguments[3], *slot)
        {
            continue;
        }
        return Some((*slot, summaries[3].oid, requested));
    }
    None
}

fn contains_integer_cast_of_parameter(expression: &ast::Expr, slot: usize) -> bool {
    if let ast::Expr::Cast {
        expr, data_type, ..
    } = expression
    {
        if matches!(data_type, DataType::SmallInt | DataType::Int) && contains_parameter(expr, slot)
        {
            return true;
        }
    }
    let mut found = false;
    let _ = expression.try_for_each_child(|child| {
        if contains_integer_cast_of_parameter(child, slot) {
            found = true;
            std::ops::ControlFlow::Break(())
        } else {
            std::ops::ControlFlow::Continue(())
        }
    });
    found
}

fn contains_parameter(expression: &ast::Expr, slot: usize) -> bool {
    if matches!(expression, ast::Expr::Param(index) if *index == slot) {
        return true;
    }
    let mut found = false;
    let _ = expression.try_for_each_child(|child| {
        if contains_parameter(child, slot) {
            found = true;
            std::ops::ControlFlow::Break(())
        } else {
            std::ops::ControlFlow::Continue(())
        }
    });
    found
}

pub(super) fn unknown_bucket_literal_operands(
    arguments: &[ast::Expr],
    summaries: &[Summary],
    fresh: [bool; 3],
) -> bool {
    if arguments.len() != 4
        || summaries.len() != 4
        || !matches!(arguments[3], ast::Expr::StringLiteral(_) | ast::Expr::Null)
    {
        return false;
    }
    (0..3).all(|index| {
        matches!(arguments[index], ast::Expr::Param(_))
            && summaries[index].qualified
            && !summaries[index].literal
            && matches!(summaries[index].oid, 0 | 705)
            && fresh[index]
    })
}
