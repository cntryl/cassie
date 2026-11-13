//! Generated-source signatures retain supplied parameter OIDs and infer unknown slots.
use super::{Fold, Frame, ParameterAdmissionError, Summary};
use crate::sql::{ast, binder, series_signature, table_sources};
use crate::types::DataType;

impl Fold<'_, '_> {
    pub(super) fn generated_function(
        &mut self,
        function: &ast::FunctionCall,
        frame: &Frame<'_>,
    ) -> Result<Option<Summary>, ParameterAdmissionError> {
        if table_sources::is_generated(&function.name)
            && self.occurrences.len() == frame.occurrence_start
        {
            // A source's sublinks and lateral columns receive their native
            // types during binding. No parameter occurred beneath this frame,
            // so leave signature diagnostics to that typed binding boundary.
            return Ok(Some(Summary::unqualified()));
        }
        if table_sources::is_unnest(&function.name) {
            if function.args.is_empty() {
                self.occurrences.undefined_function("unnest", 0, None)?;
                return Ok(Some(Summary::unqualified()));
            }
            let mut output = None;
            for summary in self.summaries.values[frame.start..]
                .iter()
                .take(function.args.len())
            {
                if matches!(summary.oid, 0 | 705) {
                    self.occurrences.ambiguous_function("unnest")?;
                    return Ok(Some(Summary::unqualified()));
                }
                let Some(DataType::Array(mut kind)) =
                    binder::parameter_data_type_for_oid(summary.oid)
                else {
                    self.occurrences.undefined_function(
                        "unnest",
                        function.args.len(),
                        Some(summary.oid),
                    )?;
                    return Ok(Some(Summary::unqualified()));
                };
                while let DataType::Array(inner) = *kind {
                    kind = inner;
                }
                output.get_or_insert(i32::try_from(kind.type_oid()).unwrap_or(705));
            }
            return Ok(Some(Summary::known(output.unwrap_or(705))));
        }
        if let Some(summary) = self.json_source(function, frame)? {
            return Ok(Some(summary));
        }
        if !table_sources::is_series(&function.name) {
            return Ok(None);
        }
        let types = self.summaries.values[frame.start..]
            .iter()
            .take(function.args.len())
            .map(|summary| {
                if matches!(summary.oid, 0 | 705) {
                    None
                } else {
                    Some(binder::parameter_data_type_for_oid(summary.oid).unwrap_or(DataType::Text))
                }
            })
            .collect::<Vec<_>>();
        let signature = match series_signature::resolve_types(&types) {
            Ok(signature) => signature,
            Err(crate::app::CassieError::AmbiguousFunction(_)) => {
                self.occurrences.ambiguous_function("generate_series")?;
                return Ok(Some(Summary::unqualified()));
            }
            Err(_) => {
                self.occurrences.undefined_function(
                    "generate_series",
                    function.args.len(),
                    None,
                )?;
                return Ok(Some(Summary::unqualified()));
            }
        };
        for (summary, target) in self.summaries.values[frame.start..]
            .iter_mut()
            .zip(signature.inputs)
        {
            if matches!(summary.oid, 0 | 705) {
                self.occurrences.coerce(
                    summary,
                    i32::try_from(target.type_oid()).unwrap_or(705),
                    false,
                )?;
            }
        }
        Ok(Some(Summary::known(
            i32::try_from(signature.output.type_oid()).unwrap_or(705),
        )))
    }

    /// JSON document sources: element expansion, record-returning functions and `JSON_TABLE`.
    fn json_source(
        &mut self,
        function: &ast::FunctionCall,
        frame: &Frame<'_>,
    ) -> Result<Option<Summary>, ParameterAdmissionError> {
        if let Some(json) = table_sources::json_element_source(&function.name) {
            // The document argument is JSON (OID 114) or JSONB (OID 3802) by function family.
            let document = if json.binary { 3802 } else { 114 };
            for summary in self.summaries.values[frame.start..]
                .iter_mut()
                .take(function.args.len())
            {
                if matches!(summary.oid, 0 | 705) {
                    self.occurrences.coerce(summary, document, false)?;
                }
            }
            return Ok(Some(Summary::known(document)));
        }
        if let Some(record) = table_sources::record_source(&function.name) {
            // Only the document slot is JSON typed; a populate base keeps its own type.
            let document = if record.binary { 3802 } else { 114 };
            if let Some(summary) = self.summaries.values[frame.start..]
                .get_mut(record.document_index())
                .filter(|summary| matches!(summary.oid, 0 | 705))
            {
                self.occurrences.coerce(summary, document, false)?;
            }
            return Ok(Some(Summary::known(document)));
        }
        if table_sources::is_json_table(&function.name) {
            // The context item is JSONB; typed PASSING values keep their own types.
            if let Some(summary) = self.summaries.values[frame.start..]
                .iter_mut()
                .next()
                .filter(|summary| matches!(summary.oid, 0 | 705))
            {
                self.occurrences.coerce(summary, 3802, false)?;
            }
            // PostgreSQL resolves an untyped PASSING value to text (OID 25).
            for summary in self.summaries.values[frame.start..]
                .iter_mut()
                .take(function.args.len())
                .skip(1)
            {
                if matches!(summary.oid, 0 | 705) {
                    self.occurrences.coerce(summary, 25, false)?;
                }
            }
            return Ok(Some(Summary::known(3802)));
        }
        Ok(None)
    }
}
