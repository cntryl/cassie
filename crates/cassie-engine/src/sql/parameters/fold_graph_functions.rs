//! An undeclared parameter at an argument of a `cassie_graph` function takes the reserved
//! type the function declares (#1268), as `graph_weighted_paths` arguments do. The
//! database's reserved type identities come from the catalog the fold already holds.
use super::{Fold, Frame, ParameterAdmissionError, Summary};
use crate::sql::ast;
use crate::sql::functions::graph_types;

/// `jsonb`.
const JSONB: i32 = 3802;
/// `numeric[]`.
const NUMERIC_ARRAY: i32 = 1231;

impl Fold<'_, '_> {
    /// The declared argument OIDs of the `cassie_graph` function `name`; `None` for a
    /// function that has no reserved-type input or when the database has no graph schema.
    fn graph_argument_oids(&self, name: &str) -> Option<Vec<i32>> {
        let descriptors = self
            .scope
            .and_then(|scope| self.catalog.graph_descriptors(scope.database))?;
        let reference = i32::try_from(descriptors.element_ref.oid).ok()?;
        let path = i32::try_from(descriptors.path.oid).ok()?;
        match name.rsplit('.').next()?.to_ascii_lowercase().as_str() {
            "path_to_jsonb" | "path_steps" => Some(vec![path]),
            "key_parts" => Some(vec![reference]),
            "weighted_path_steps" => Some(vec![path, NUMERIC_ARRAY]),
            _ => None,
        }
    }

    /// Resolves the untyped arguments of `path_to_jsonb`, `key_parts`, `path_steps` and
    /// `weighted_path_steps`; `None` for any other function.
    pub(super) fn graph_function(
        &mut self,
        function: &ast::FunctionCall,
        frame: &Frame<'_>,
    ) -> Result<Option<Summary>, ParameterAdmissionError> {
        let scalar = function
            .name
            .eq_ignore_ascii_case("cassie_graph.path_to_jsonb");
        if !scalar && !graph_types::is_table_function(&function.name) {
            return Ok(None);
        }
        let declared = self.graph_argument_oids(&function.name).unwrap_or_default();
        if declared.is_empty() || declared.len() != function.args.len() {
            // A database without the graph schema, or a call of the wrong arity, keeps the
            // generic admission and its diagnostics.
            return Ok(None);
        }
        for (summary, target) in self.summaries.values[frame.start..]
            .iter_mut()
            .take(function.args.len())
            .zip(declared)
        {
            if matches!(summary.oid, 0 | 705) {
                self.occurrences.coerce(summary, target, false)?;
            }
        }
        Ok(Some(if scalar {
            Summary::known(JSONB)
        } else {
            Summary::unqualified()
        }))
    }
}
