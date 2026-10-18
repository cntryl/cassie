//! PostgreSQL 18 grammar and expression contracts tracked by #796.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[path = "grammar/default_unicode_identifier.rs"]
mod default_unicode_identifier;
#[path = "grammar/operators/integer_bitwise_operator_precedence.rs"]
mod integer_bitwise_operator_precedence;

type ValuePredicate = fn(&Value) -> bool;

include!("grammar/ddl/parameters.rs");

include!("grammar/parameters.rs");

include!("grammar/metadata.rs");

include!("grammar/semantics.rs");

include!("grammar/parameters/parameters.rs");

include!("grammar/functions/string/semantics.rs");

include!("grammar/predicates/semantics.rs");

include!("grammar/operators/semantics.rs");

include!("grammar/patterns/semantics.rs");

include!("grammar/resources.rs");

include!("grammar/predicates/nulls.rs");

include!("grammar/relations/semantics.rs");

include!("grammar/types/numeric/semantics.rs");

include!("grammar/operators/binary.rs");

include!("grammar/functions/math/semantics.rs");

include!("grammar/types/arrays/semantics.rs");

include!("grammar/errors.rs");
