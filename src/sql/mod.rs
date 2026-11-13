pub(crate) mod check_comparison;
pub(crate) mod literal_inputs;
pub(crate) mod parse_analysis;
use std::collections::HashMap;

use crate::catalog::FieldMeta;
use crate::types::DataType;

pub mod ast;
pub mod binder;
pub(crate) mod catalog_scope;
mod column_identifier;
pub(crate) mod definition_guard;
pub mod functions;
mod identifier_text;
pub(crate) mod numeric_comparison;
pub(crate) mod pagination;
mod parameters;
pub mod parser;
pub(crate) mod series_signature;
mod source_identity;
mod source_types;
pub(crate) mod table_sources;
pub(crate) mod timetz_format_signature;
pub(crate) use source_identity::physical_collection;
pub(crate) use source_types::{
    source_field_type_map_with_ctes, source_field_type_map_with_parameters,
};

pub use ast::{
    AlterIndexOperation, AlterIndexStatement, AlterSchemaOperation, AlterSchemaStatement,
    AlterTableOperation, AlterTableStatement, CatalogStatement, CatalogStatementRef,
    CommonTableExpression, CopyFormat, CopyStatement, CreateMaterializedViewStatement,
    CreateSchemaStatement, CreateTableLikeClause, CreateTableLikeFeature, CreateTableLikeOption,
    CreateTableStatement, CreateViewStatement, DeleteStatement, DropMaterializedViewStatement,
    DropSchemaStatement, DropTableStatement, DropViewStatement, FieldDefinition,
    IdentifierComponent, IdentifierPath, InsertStatement, ParsedStatement, ProjectionStatement,
    ProjectionStatementRef, QuerySource, QueryStatement, RefreshMaterializedViewStatement,
    ReindexStatement, ReindexTarget, RenameMaterializedViewStatement, RetentionStatement,
    RetentionStatementRef, RuntimeStatement, RuntimeStatementRef, SelectItem, SelectStatement,
    StatementFamily, StatementRoute, StatementRouteRef, UpdateStatement,
};
pub use binder::{bind, BoundStatement};
pub(crate) use catalog_scope::parse_statement_in_catalog;
pub(crate) use column_identifier::ColumnIdentifierPath;
pub use functions::registry;
pub use parser::{
    parse_statement, with_domain_scope, with_enum_scope, with_type_scope, DomainResolver,
    EnumResolver, SqlError, SqlErrorKind, TypeResolver,
};

#[cfg(test)]
pub(crate) use parameters::checked_parse_parameter_types_in_scope;
pub(crate) use parameters::AdmittedParameterTypes;
pub(crate) use parameters::{
    checked_parameter_types_in_scope, checked_wire_parse_parameter_types_in_scope,
    contextual_string_parameter_type_oids, validate_wire_parse_parameter_slots,
    ParameterAdmissionError, ParameterSemanticDiagnostic, PrivateParameterTypeInput,
    ScalarLiteralDiagnostic,
};
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
        ast::Expr::Cast { expr, .. } | ast::Expr::Collate { expr, .. } => {
            column_expr_type(expr, field_types)
        }
        _ => None,
    }
}

pub(crate) fn field_type_for_column<'a>(
    field_types: &'a FieldTypeMap,
    column: &str,
) -> Option<&'a DataType> {
    let column = crate::sql::ColumnIdentifierPath::parse(column).ok()?;
    let found = field_types
        .get(&column.namespace_key())
        .or_else(|| field_types.get(&column.lookup_key()));
    if found.is_some() {
        return found;
    }
    // A row carrier that this source does not declare is an enclosing query's alias. Its
    // bare field name may name a different column here (generated sources also call their
    // output `column1`), so it must not resolve by name alone.
    if column.is_qualified()
        && column
            .namespace_qualifier()
            .is_some_and(|qualifier| qualifier.starts_with("__cassie_relation_alias_"))
    {
        return None;
    }
    field_types.get(&column.field_lookup_key())
}

#[cfg(test)]
mod parse_analysis_tests;
