//! Residual PostgreSQL array search function behavior for #807.

use super::super::support_pgwire as wire;
use super::super::support_type_metadata_contract as fixture;

include!("null_search.rs");

include!("search_positions.rs");
