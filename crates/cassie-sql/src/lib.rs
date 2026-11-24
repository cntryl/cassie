//! Cassie's SQL syntax model and parser.

pub mod ast;
pub mod attachment;
pub use cassie_core::{catalog, types};
pub mod column_identifier;
pub mod composite_unique;
pub mod catalog_declarations;
pub mod definition_guard;
pub mod default_expression;
pub mod identifier_text;
pub mod index_options;
pub mod maintenance_declarations;
pub mod parser;
mod parameter_count;
pub mod rollup_declarations;
pub mod routine_declarations;
pub mod sequence_default;
pub mod type_publication;

#[cfg(test)]
mod ast_attachment_tests;
pub mod declarations;
pub mod registry;

pub use parameter_count::parameter_count;
pub use registry::{functions, table_sources};

pub use parser::{
    parse_statement, with_domain_scope, with_enum_scope, with_type_scope, DomainResolver,
    EnumResolver, SqlError, SqlErrorKind, TypeResolver,
};

/// Compatibility namespace for syntax consumers moving from the engine SQL module.
pub mod sql {
    pub use crate::ast;
    pub use crate::ast::*;
    pub use crate::functions;
    pub use crate::parser;
    pub use crate::parse_statement;
    pub use crate::parameter_count;
}
