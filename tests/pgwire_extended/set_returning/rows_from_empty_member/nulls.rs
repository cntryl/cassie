//! `ROWS FROM` NULL padding when one member returns no rows.

use crate::support_parameter_occurrence_wire::{run_script, Step};
use crate::support_pgwire as wire;
