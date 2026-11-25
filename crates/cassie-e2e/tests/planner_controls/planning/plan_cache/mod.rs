use crate::support_sql::use_local_storage;
use cassie::app::Cassie;
use cassie::catalog::canonical_relation_name;
use cassie::config::CassieRuntimeConfig;
use cassie::runtime::ExecutionMode;
use cassie::sql::parse_statement;
use cassie::types::{DataType, FieldSchema, Schema};

use super::support_metrics as support;
use support::*;

fn adaptive_execution_config() -> CassieRuntimeConfig {
    let mut config = CassieRuntimeConfig::default();
    config.limits.adaptive_execution_enabled = true;
    config.limits.adaptive_min_cost_savings_bps = 100;
    config
}

include!("parameters.rs");

include!("semantics.rs");

include!("errors.rs");

include!("restart.rs");

include!("transactions.rs");
