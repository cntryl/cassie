//! PostgreSQL 18 parser, relation, collation, and wire contracts tracked by #796 and #800.

use std::time::Duration;

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;
use tokio::io::AsyncWriteExt;

#[path = "grammar/row_ordering_null_stopping.rs"]
mod row_ordering_null_stopping;
#[path = "patterns/unicode_identifier/resources.rs"]
mod unicode_identifier_residuals;
#[path = "grammar/unsupported_type_lookup.rs"]
mod unsupported_type_lookup;
#[path = "grammar/values_typed_source_descriptors.rs"]
mod values_typed_source_descriptors;

struct GrammarFixtureDirectory(String);

impl Drop for GrammarFixtureDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

include!("grammar/parameters.rs");

include!("grammar/semantics.rs");

include!("grammar/resources.rs");

include!("grammar/errors.rs");

include!("grammar/metadata.rs");

include!("aggregates/grouping/semantics.rs");

include!("grammar/cancellation.rs");
