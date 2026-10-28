//! Timestamp-without-time-zone series retains its PostgreSQL result identity (#808).

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;
