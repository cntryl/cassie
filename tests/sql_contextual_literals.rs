use cassie_test_support::support_sql;

use cassie::app::CassieError;
use cassie::types::Value;

include!("sql_contextual_literals/contracts/errors.rs");

include!("sql_contextual_literals/contracts/semantics.rs");
