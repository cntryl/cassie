//! PostgreSQL 18.6 four-argument `timestamptz` `generate_series` wire contract for #808.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;

const TSTZ_TIMEZONE_SERIES_SQL: &str = "SELECT value FROM generate_series(\
    $1::TIMESTAMPTZ, $2::TIMESTAMPTZ, $3::INTERVAL, $4::TEXT) AS series(value)";
