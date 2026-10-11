use std::collections::HashMap;

use crate::catalog::FieldMeta;
use crate::types::DataType;

pub mod ast;
pub mod binder;
mod column_identifier;
pub(crate) mod definition_guard;
pub mod functions;
pub(crate) mod pagination;
mod parameters;
pub mod parser;
mod source_identity;
mod source_types;
pub(crate) use source_identity::physical_collection;
pub(crate) use source_types::source_field_type_map_with_ctes;

pub use ast::{
    AlterSchemaOperation, AlterSchemaStatement, AlterTableOperation, AlterTableStatement,
    CatalogStatement, CatalogStatementRef, CommonTableExpression, CopyFormat, CopyStatement,
    CreateSchemaStatement, CreateTableStatement, CreateViewStatement, DeleteStatement,
    DropSchemaStatement, DropTableStatement, DropViewStatement, FieldDefinition,
    IdentifierComponent, IdentifierPath, InsertStatement, ParsedStatement, ProjectionStatement,
    ProjectionStatementRef, QuerySource, QueryStatement, RetentionStatement, RetentionStatementRef,
    RuntimeStatement, RuntimeStatementRef, SelectItem, SelectStatement, StatementFamily,
    StatementRoute, StatementRouteRef, UpdateStatement,
};
pub use binder::{bind, BoundStatement};
pub(crate) use column_identifier::ColumnIdentifierPath;
pub use functions::registry;
pub use parser::{parse_statement, SqlError, SqlErrorKind};

pub(crate) use parameters::{checked_parse_parameter_types, ParameterAdmissionError};
pub use parameters::{
    declared_parameter_type_conflict, parameter_count, parameter_type_oids,
    parameter_type_oids_with_catalog,
};

pub(crate) type FieldTypeMap = HashMap<String, DataType>;

pub(crate) fn source_field_type_map(
    source: &ast::QuerySource,
    catalog: &crate::catalog::Catalog,
) -> FieldTypeMap {
    source_field_type_map_with_ctes(source, &[], catalog)
}

fn infer_projected_field_types(
    statement: &QueryStatement,
    catalog: &crate::catalog::Catalog,
    fields: &mut FieldTypeMap,
) {
    if let QueryStatement::Select(select) = statement {
        fields.extend(source_field_type_map(&select.source, catalog));
    }
}

fn field_type_map<'a>(fields: impl IntoIterator<Item = &'a FieldMeta>) -> FieldTypeMap {
    fields
        .into_iter()
        .map(|field| {
            (
                crate::sql::ColumnIdentifierPath::from_field_name(&field.name).lookup_key(),
                field.data_type.clone(),
            )
        })
        .collect()
}

fn column_expr_type<'a>(expr: &ast::Expr, field_types: &'a FieldTypeMap) -> Option<&'a DataType> {
    match expr {
        ast::Expr::Column(column) => field_type_for_column(field_types, column),
        ast::Expr::Cast { expr, .. } => column_expr_type(expr, field_types),
        _ => None,
    }
}

pub(crate) fn field_type_for_column<'a>(
    field_types: &'a FieldTypeMap,
    column: &str,
) -> Option<&'a DataType> {
    let column = crate::sql::ColumnIdentifierPath::parse(column).ok()?;
    field_types
        .get(&column.namespace_key())
        .or_else(|| field_types.get(&column.lookup_key()))
        .or_else(|| field_types.get(&column.field_lookup_key()))
}
