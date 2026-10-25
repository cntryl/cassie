//! PostgreSQL JSON set-returning and record conversion behavior.

use crate::support_pgwire as wire;
use crate::support_type_metadata_contract as fixture;
use cassie::types::Value;
