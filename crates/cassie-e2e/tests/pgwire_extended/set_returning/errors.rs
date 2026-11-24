//! PostgreSQL 18 set-returning function error contracts tracked by #808.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;

#[path = "parameterized_unnest.rs"]
mod parameterized_unnest;
#[path = "../types/temporal/timestamptz_timezone_series/parameters.rs"]
mod timestamptz_timezone_series;
