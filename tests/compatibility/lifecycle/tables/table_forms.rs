use super::*;

#[path = "table_forms/create_table_as.rs"]
mod create_table_as_contracts;
#[path = "materialized_view_create_no_data.rs"]
mod materialized_view_create_no_data;
#[path = "explicit_table_clone.rs"]
mod postgresql18_explicit_table_clone_contracts;
#[path = "lifecycle.rs"]
mod postgresql18_lifecycle_gaps;

include!("table_forms/metadata.rs");

include!("table_forms/parameters.rs");

include!("table_forms/resources.rs");

#[path = "table_forms/like_options.rs"]
mod like_options;

include!("table_forms/restart.rs");

include!("table_forms/semantics.rs");
