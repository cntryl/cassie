//! Apply borrowed input signatures to already transformed argument occurrences.

use crate::app::CassieError;
use crate::sql::functions::FunctionInputSignature;
use crate::sql::{ast, functions};
use crate::types::DataType;

use super::check_controls;
use super::expressions::{common, oid};
use super::{Fold, Frame, ParameterAdmissionError, Summary};

impl Fold<'_, '_> {
    pub(super) fn function(
        &mut self,
        function: &ast::FunctionCall,
        frame: &Frame<'_>,
    ) -> Result<Summary, ParameterAdmissionError> {
        if let Some(oid) = functions::hashing::return_type(&function.name)
            .and_then(|kind| i32::try_from(kind.type_oid()).ok())
        {
            return Ok(Summary::known(oid));
        }
        if functions::function(&function.name).is_some_and(|metadata| {
            functions::is_bytea_function(metadata.id) || metadata.id == functions::FunctionId::Lower
        }) {
            if let Some(summary) = self.declared_function_summary(function, frame)? {
                Self::reject_nonaggregate_modifier(function, self.controls)?;
                return Ok(summary);
            }
        }
        if let Some(summary) = self.fixed_bytea_function(function, frame)? {
            Self::reject_nonaggregate_modifier(function, self.controls)?;
            return Ok(summary);
        }
        let common_parent = self.common_function_parent()?;
        let boolean_metadata = functions::function(&function.name).filter(|metadata| {
            matches!(
                functions::input_signature(metadata.id),
                Some(FunctionInputSignature::Boolean)
            )
        });
        if let Some(summary) = self.catalog_arity(function)? {
            return Ok(summary);
        }
        if function.args.is_empty()
            && !common_parent
            && boolean_metadata.is_none()
            && !function.has_modifiers()
        {
            return Ok(Summary::unqualified());
        }
        let declared = self.declared_function_summary(function, frame)?;
        if let Some(summary) = declared {
            Self::reject_nonaggregate_modifier(function, self.controls)?;
            if !summary.qualified {
                self.occurrences
                    .unqualify_since(frame.occurrence_start, self.controls)?;
            }
            return Ok(summary);
        }
        if let Some(metadata) = boolean_metadata {
            let star = matches!(function.args.as_slice(), [ast::Expr::Column(name)] if name == "*");
            if function.args.len() != 1 || star {
                self.occurrences.undefined_function(
                    metadata.name,
                    if star { 0 } else { function.args.len() },
                    None,
                )?;
                return Ok(Summary::known(16));
            }
        }
        if let Some(summary) = self.type_function_summary(function, frame)? {
            return Ok(summary);
        }
        let metadata = functions::function(&function.name)
            .filter(|metadata| metadata.arity.matches(function.args.len()));
        if metadata.is_some_and(|metadata| !metadata.is_aggregate) {
            Self::reject_nonaggregate_modifier(function, self.controls)?;
        }
        if metadata.is_some_and(|metadata| metadata.is_aggregate)
            && function.is_distinct()
            && function.order_by.iter().any(|order| {
                !function
                    .args
                    .iter()
                    .any(|argument| argument.structurally_eq(&order.expr))
            })
        {
            self.occurrences.invalid_distinct_order()?;
        }
        if let Some(metadata) = metadata.filter(|metadata| metadata.is_aggregate) {
            self.validate_aggregate_inputs(function, metadata, frame)?;
        }
        if let Some(summary) = self.catalog_input(function, metadata, frame)? {
            return Ok(summary);
        }
        self.apply_input_signature(function, metadata, frame)
    }

    fn type_function_summary(
        &mut self,
        function: &ast::FunctionCall,
        frame: &Frame<'_>,
    ) -> Result<Option<Summary>, ParameterAdmissionError> {
        let Some(cast) = functions::primitive_casts::candidate(function) else {
            return Ok(None);
        };
        if let ast::Expr::StringLiteral(input) = &function.args[0] {
            self.validate_type_function_literal(input, &cast.data_type)?;
        }
        let source = self.summaries.values[frame.start].oid;
        // Unknown constants invoke type coercion; unresolved parameters still
        // participate in native overload selection.
        if !matches!(function.args[0], ast::Expr::Param(_)) {
            return Ok(Some(Summary::known(oid(&cast.data_type))));
        }
        if matches!(source, 0 | 705) {
            match cast.data_type {
                DataType::Uuid => self
                    .occurrences
                    .undefined_function(cast.name, 1, Some(705))?,
                DataType::Date | DataType::Timestamp => {
                    self.occurrences.pending_type_function_input()?;
                }
                DataType::Text | DataType::Varchar { .. } => {
                    self.occurrences.coerce(
                        &mut self.summaries.values[frame.start],
                        oid(&cast.data_type),
                        true,
                    )?;
                }
                _ => self.occurrences.ambiguous_function(cast.name)?,
            }
        }
        Ok(Some(Summary::known(oid(&cast.data_type))))
    }

    fn apply_input_signature(
        &mut self,
        function: &ast::FunctionCall,
        metadata: Option<&functions::ScalarFunction>,
        frame: &Frame<'_>,
    ) -> Result<Summary, ParameterAdmissionError> {
        let signature = self.qualified_input_signature(function, metadata, frame);
        match signature {
            Some(FunctionInputSignature::Positional(oids)) => self.positional_function(
                function,
                metadata.expect("positional metadata"),
                oids,
                frame,
            ),
            Some(FunctionInputSignature::Boolean) => {
                for (argument, summary) in function
                    .args
                    .iter()
                    .zip(&mut self.summaries.values[frame.start..])
                {
                    check_controls(self.controls)?;
                    self.occurrences.coerce(summary, 16, true)?;
                    self.occurrences.boolean_aggregate_input(
                        metadata.expect("Boolean signature metadata").name,
                        *summary,
                    )?;
                    if summary.oid == 705 {
                        if let ast::Expr::StringLiteral(input) = argument {
                            self.occurrences
                                .boolean_aggregate_literal(input, self.controls)?;
                        }
                    }
                }
                Ok(Summary::known(16))
            }
            Some(FunctionInputSignature::Text) => {
                for summary in &mut self.summaries.values[frame.start..] {
                    check_controls(self.controls)?;
                    self.occurrences.coerce(summary, 25, true)?;
                }
                Ok(Summary::known(25))
            }
            Some(FunctionInputSignature::PercentileFraction) => {
                for summary in &mut self.summaries.values[frame.start..] {
                    check_controls(self.controls)?;
                    if summary.oid != 1022 {
                        self.occurrences.coerce(summary, 701, true)?;
                    }
                }
                Ok(Summary::unqualified())
            }
            Some(FunctionInputSignature::CommonArguments) => self.common_function(frame),
            Some(FunctionInputSignature::OrderedSetOrderKey) => {
                Ok(self.ordered_set_order_key(function, frame))
            }
            Some(FunctionInputSignature::HypotheticalSetArguments) => {
                self.hypothetical_set_arguments(function, metadata, frame)
            }
            None => {
                self.occurrences
                    .unqualify_since(frame.occurrence_start, self.controls)?;
                Ok(Summary::unqualified())
            }
        }
    }

    fn ordered_set_order_key(&self, function: &ast::FunctionCall, frame: &Frame<'_>) -> Summary {
        let Some(order) = function.order_by.first() else {
            return Summary::unqualified();
        };
        let Some(oid) = self.ordered_set_order_key_oid(&order.expr, frame) else {
            return Summary::unqualified();
        };
        if oid == 0 || oid == 705 {
            Summary::unqualified()
        } else {
            Summary::known(oid)
        }
    }

    fn hypothetical_set_arguments(
        &mut self,
        function: &ast::FunctionCall,
        metadata: Option<&functions::ScalarFunction>,
        frame: &Frame<'_>,
    ) -> Result<Summary, ParameterAdmissionError> {
        if function.args.len() == function.order_by.len() {
            for (index, order) in function.order_by.iter().enumerate() {
                check_controls(self.controls)?;
                let Some(oid) = self.ordered_set_order_key_oid(&order.expr, frame) else {
                    continue;
                };
                if oid == 0 || oid == 705 {
                    continue;
                }
                let Some(direct) = self.summaries.values.get_mut(frame.start + index) else {
                    continue;
                };
                self.occurrences.coerce(direct, oid, true)?;
            }
        }
        Ok(match metadata.map(|metadata| metadata.return_type) {
            Some(functions::FunctionReturnType::BigInt) => Summary::known(20),
            Some(functions::FunctionReturnType::Float) => Summary::known(701),
            _ => Summary::unqualified(),
        })
    }

    fn ordered_set_order_key_oid(&self, expression: &ast::Expr, frame: &Frame<'_>) -> Option<i32> {
        let context = self.contexts.values.get(frame.context)?;
        crate::sql::binder::known_expr_type(expression, &context.fields)
            .map(|data_type| oid(&data_type))
    }

    fn catalog_arity(
        &mut self,
        function: &ast::FunctionCall,
    ) -> Result<Option<Summary>, ParameterAdmissionError> {
        if let Some(metadata) = functions::function(&function.name).filter(|metadata| {
            functions::is_runtime_helper(metadata.id)
                || functions::is_bytea_function(metadata.id)
                || matches!(
                    metadata.id,
                    functions::FunctionId::PgSizeBytes
                        | functions::FunctionId::PgSizePretty
                        | functions::FunctionId::PgCharToEncoding
                )
        }) {
            if !metadata.arity.matches(function.args.len()) {
                self.occurrences
                    .undefined_function(metadata.name, function.args.len(), None)?;
                return Ok(Some(Summary::unqualified()));
            }
        }
        Ok(None)
    }

    fn catalog_input(
        &mut self,
        function: &ast::FunctionCall,
        metadata: Option<&functions::ScalarFunction>,
        frame: &Frame<'_>,
    ) -> Result<Option<Summary>, ParameterAdmissionError> {
        if metadata.is_some_and(|value| {
            matches!(
                value.id,
                functions::FunctionId::PgColumnCompression | functions::FunctionId::PgColumnSize
            )
        }) {
            if let Some(ast::Expr::Param(slot)) = function.args.first() {
                if matches!(self.summaries.values[frame.start].oid, 0 | 705) {
                    self.occurrences.unresolved_parameter(*slot)?;
                    return Ok(Some(Summary::unqualified()));
                }
                if metadata.is_some_and(|value| value.id == functions::FunctionId::PgColumnSize) {
                    self.occurrences.pending_datum_input()?;
                    return Ok(Some(Summary::unqualified()));
                }
            }
        }
        if metadata.is_some_and(|value| value.id == functions::FunctionId::PgCharToEncoding)
            && matches!(function.args.first(), Some(ast::Expr::Param(_)))
            && matches!(self.summaries.values[frame.start].oid, 0 | 705)
        {
            // Native inference chooses NAME (OID19), whose carrier is pending.
            // Do not advertise TEXT as a substitute parameter identity.
            self.occurrences.pending_name_input()?;
            return Ok(Some(Summary::unqualified()));
        }
        if let Some(metadata) =
            metadata.filter(|value| value.id == functions::FunctionId::PgSizePretty)
        {
            let argument = self.summaries.values[frame.start];
            if argument.oid == 705 || argument.oid == 0 {
                self.occurrences.ambiguous_function(metadata.name)?;
                return Ok(Some(Summary::unqualified()));
            }
            // The NUMERIC overload awaits its actual runtime carrier. Do not
            // coerce that accepted native signature into an integer overload.
            if argument.oid == 1700 {
                self.occurrences
                    .unqualify_since(frame.occurrence_start, self.controls)?;
                return Ok(Some(Summary::unqualified()));
            }
        }
        Ok(None)
    }

    fn qualified_input_signature(
        &self,
        function: &ast::FunctionCall,
        metadata: Option<&functions::ScalarFunction>,
        frame: &Frame<'_>,
    ) -> Option<FunctionInputSignature> {
        if function
            .name
            .rsplit('.')
            .next()
            .is_some_and(|name| name.eq_ignore_ascii_case("substring"))
            && metadata.is_some()
            && function.args.len() >= 2
            && !self.summaries.values[frame.start + 1..]
                .iter()
                .any(|summary| {
                    matches!(summary.oid, 21 | 23)
                        || summary.oid == 20 && self.occurrences.embedded_int4_carrier(*summary)
                })
        {
            Some(FunctionInputSignature::Positional(&[25, 25, 25]))
        } else if let Some(functions::FunctionId::Math(math)) = metadata.map(|metadata| metadata.id)
        {
            functions::math::input_oids(math, function.args.len())
                .map(FunctionInputSignature::Positional)
        } else {
            metadata.and_then(|metadata| functions::input_signature(metadata.id))
        }
    }

    fn positional_function(
        &mut self,
        function: &ast::FunctionCall,
        metadata: &functions::ScalarFunction,
        oids: &[i32],
        frame: &Frame<'_>,
    ) -> Result<Summary, ParameterAdmissionError> {
        // `power(numeric, numeric)` is its own overload.
        if metadata.id == functions::FunctionId::Power
            && self.summaries.values[frame.start..]
                .iter()
                .take(function.args.len())
                .any(|summary| summary.oid == 1700)
        {
            return Ok(Summary::known(1700));
        }
        for (summary, oid) in self.summaries.values[frame.start..].iter_mut().zip(oids) {
            check_controls(self.controls)?;
            let accepted = match *oid {
                17 => matches!(summary.oid, 0 | 17 | 705),
                25 => matches!(summary.oid, 0 | 25 | 1042 | 1043 | 705),
                20 => matches!(summary.oid, 0 | 20 | 21 | 23 | 705),
                701 => matches!(summary.oid, 0 | 20 | 21 | 23 | 700 | 701 | 705 | 1700),
                // Float arguments reach the binder, which names the full
                // undefined `numeric` overload signature.
                1700 => matches!(summary.oid, 0 | 20 | 21 | 23 | 700 | 701 | 705 | 1700),
                _ => {
                    matches!(summary.oid, 0 | 21 | 23 | 705)
                        || summary.oid == 20 && self.occurrences.embedded_int4_carrier(*summary)
                }
            };
            if !accepted && summary.qualified {
                self.occurrences.undefined_function(
                    metadata.name,
                    function.args.len(),
                    Some(summary.oid),
                )?;
            }
            self.occurrences.coerce(summary, *oid, true)?;
        }
        if let functions::FunctionId::Math(math) = metadata.id {
            let arguments = self.summaries.values[frame.start..]
                .iter()
                .take(function.args.len())
                .map(|summary| {
                    u32::try_from(summary.oid)
                        .ok()
                        .and_then(crate::types::builtin::parameter_builtin_for_oid)
                        .map_or(DataType::Null, |metadata| metadata.kind.data_type())
                })
                .collect::<Vec<_>>();
            let result = functions::math::result_type(math, &arguments);
            return Ok(Summary::known(
                i32::try_from(result.type_oid()).unwrap_or_default(),
            ));
        }
        Ok(Summary::known(match metadata.return_type {
            functions::FunctionReturnType::Float => 701,
            functions::FunctionReturnType::Int => 23,
            functions::FunctionReturnType::BigInt => 20,
            functions::FunctionReturnType::Boolean => 16,
            functions::FunctionReturnType::Bytea => 17,
            _ => 25,
        }))
    }

    fn validate_aggregate_inputs(
        &mut self,
        function: &ast::FunctionCall,
        metadata: &functions::ScalarFunction,
        frame: &Frame<'_>,
    ) -> Result<(), ParameterAdmissionError> {
        for summary in &self.summaries.values[frame.start..] {
            if summary.oid == 0 || summary.oid == 705 {
                continue;
            }
            let Some(data_type) = u32::try_from(summary.oid)
                .ok()
                .and_then(crate::types::builtin::parameter_builtin_for_oid)
                .map(|metadata| metadata.kind.data_type())
            else {
                continue;
            };
            if !functions::aggregate_accepts_input(metadata.id, &data_type) {
                self.occurrences.undefined_function(
                    metadata.name,
                    function.args.len(),
                    Some(summary.oid),
                )?;
            }
            if function.is_distinct() && !functions::has_distinct_equality(&data_type) {
                self.occurrences.missing_equality(summary.oid)?;
            }
        }
        Ok(())
    }

    fn common_function_parent(&self) -> Result<bool, ParameterAdmissionError> {
        let mut common_parent = false;
        for frame in self.frames.values.iter().rev() {
            check_controls(self.controls)?;
            match frame.node {
                super::Node::Expr(ast::Expr::Function(_)) => continue,
                super::Node::Function(function) => {
                    common_parent = functions::function(&function.name).is_some_and(|metadata| {
                        matches!(
                            functions::input_signature(metadata.id),
                            Some(FunctionInputSignature::CommonArguments)
                        )
                    });
                }
                _ => {}
            }
            break;
        }
        Ok(common_parent)
    }

    fn declared_function_summary(
        &mut self,
        function: &ast::FunctionCall,
        frame: &Frame<'_>,
    ) -> Result<Option<Summary>, ParameterAdmissionError> {
        let occurrences = &mut self.occurrences;
        let summaries = &mut self.summaries.values[frame.start..];
        self.catalog
            .with_function_signature(
                &function.name,
                self.scope,
                Some(self.controls),
                |signature| {
                    let Some((arguments, result)) = signature else {
                        return Ok(None);
                    };
                    if functions::has_distinct_builtin_arity(
                        &function.name,
                        function.args.len(),
                        arguments.len(),
                    ) {
                        return Ok(None);
                    }
                    if let Some(cast) = functions::primitive_casts::candidate(function) {
                        if arguments.len() == 1 && summaries.len() == 1 {
                            let source = summaries[0].oid;
                            if !matches!(source, 0 | 705) && source != oid(&arguments[0].data_type)
                            {
                                return Ok(None);
                            }
                            if matches!(source, 0 | 705)
                                && cast.data_type != DataType::Uuid
                                && !matches!(
                                    arguments[0].data_type,
                                    DataType::Text
                                        | DataType::Varchar { .. }
                                        | DataType::Char { .. }
                                )
                            {
                                occurrences.pending_type_function_input()?;
                                return Ok(Some(Summary::unqualified()));
                            }
                        }
                    }
                    let mut qualified = arguments.len() == summaries.len();
                    for argument in arguments {
                        check_controls(self.controls)?;
                        qualified &= primitive(&argument.data_type);
                    }
                    if qualified {
                        for (summary, argument) in summaries.iter_mut().zip(arguments) {
                            check_controls(self.controls)?;
                            occurrences.coerce(summary, oid(&argument.data_type), true)?;
                        }
                    }
                    Ok(Some(if qualified && primitive(result) {
                        Summary::known(oid(result))
                    } else {
                        Summary::unqualified()
                    }))
                },
            )
            .map_err(admission_error)?
    }

    fn reject_nonaggregate_modifier(
        function: &ast::FunctionCall,
        controls: &crate::runtime::QueryExecutionControls,
    ) -> Result<(), ParameterAdmissionError> {
        let modifier = if function.is_distinct() {
            Some("DISTINCT")
        } else if !function.order_by.is_empty() {
            Some("ORDER BY")
        } else if function.filter.is_some() {
            Some("FILTER")
        } else {
            None
        };
        if let Some(modifier) = modifier {
            return Err(super::super::admission::admit_nonaggregate_modifier(
                &function.name,
                modifier,
                controls,
            )?);
        }
        Ok(())
    }

    pub(super) fn invalid_boolean_function_literals(
        &self,
        function: &ast::FunctionCall,
        summary: Summary,
        frame: &Frame<'_>,
    ) -> Result<(), ParameterAdmissionError> {
        if summary.oid != 16
            || !functions::function(&function.name).is_some_and(|metadata| {
                matches!(
                    functions::input_signature(metadata.id),
                    Some(FunctionInputSignature::CommonArguments)
                )
            })
        {
            return Ok(());
        }
        for (argument, argument_summary) in function
            .args
            .iter()
            .zip(&self.summaries.values[frame.start..])
        {
            check_controls(self.controls)?;
            if argument_summary.oid == 705 {
                if let ast::Expr::StringLiteral(input) = argument {
                    self.occurrences
                        .invalid_boolean_literal(input, self.controls)?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn values_columns(
        &mut self,
        rows: &[Vec<ast::Expr>],
        frame: &Frame<'_>,
    ) -> Result<(), ParameterAdmissionError> {
        let width = rows.first().map_or(0, Vec::len);
        // Transform every row before selecting common column types. In
        // particular, repeated unknown parameters retain their creation OID
        // until column coercion reports a conflicting demand as 42P08.
        for column in 0..width {
            let mut result = Summary::literal();
            for row in 0..rows.len() {
                check_controls(self.controls)?;
                let summary = self.summaries.values[frame.start + row * width + column];
                if !summary.qualified {
                    result = Summary::unqualified();
                    break;
                }
                let next = if matches!(result.oid, 25 | 1042 | 1043)
                    && matches!(summary.oid, 25 | 1042 | 1043)
                {
                    Some(result.oid)
                } else {
                    common(result, summary)
                };
                let Some(next) = next else {
                    self.occurrences.values_types(result.oid, summary.oid)?;
                    result = Summary::unqualified();
                    break;
                };
                result = Summary::known(next);
            }
            if !result.qualified {
                continue;
            }
            if matches!(result.oid, 0 | 705) {
                result = Summary::known(25);
            }
            for row in 0..rows.len() {
                check_controls(self.controls)?;
                let summary = &mut self.summaries.values[frame.start + row * width + column];
                self.occurrences.coerce(summary, result.oid, true)?;
            }
        }
        Ok(())
    }

    fn common_function(&mut self, frame: &Frame<'_>) -> Result<Summary, ParameterAdmissionError> {
        let mut result = Summary::literal();
        for summary in &self.summaries.values[frame.start..] {
            check_controls(self.controls)?;
            if !summary.qualified {
                self.occurrences
                    .unqualify_since(frame.occurrence_start, self.controls)?;
                return Ok(Summary::unqualified());
            }
            let Some(next) = common(result, *summary) else {
                self.occurrences
                    .unqualify_since(frame.occurrence_start, self.controls)?;
                return Ok(Summary::unqualified());
            };
            result = Summary::known(next);
        }
        if result.oid == 705 || result.oid == 0 {
            result = Summary::known(25);
        }
        for summary in &mut self.summaries.values[frame.start..] {
            check_controls(self.controls)?;
            self.occurrences.coerce(summary, result.oid, true)?;
        }
        Ok(result)
    }
}

fn primitive(data_type: &DataType) -> bool {
    matches!(
        data_type,
        DataType::Null
            | DataType::Boolean
            | DataType::SmallInt
            | DataType::Int
            | DataType::BigInt
            | DataType::Float
            | DataType::Text
            | DataType::Char { .. }
            | DataType::Varchar { .. }
    )
}

fn admission_error(error: CassieError) -> ParameterAdmissionError {
    match error {
        CassieError::QueryCancelled => ParameterAdmissionError::Cancelled,
        CassieError::DeadlineExceeded => ParameterAdmissionError::Deadline,
        CassieError::ResourceLimit(message) => ParameterAdmissionError::Memory(message),
        other => ParameterAdmissionError::Memory(other.to_string()),
    }
}
