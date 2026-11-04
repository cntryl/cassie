//! Bind USING cast provenance against the current per-operation schema.
use super::{AlterTableOperation, BindingContext, CassieError, Catalog, CollectionSchema};
use crate::sql::binder::{boolean_contexts, coalesce_results::ResultTypes};
use crate::types::FieldSchema;

pub(super) fn bind(
    operation: &mut AlterTableOperation,
    source_table: &str,
    table: &str,
    schema: &CollectionSchema,
    catalog: &Catalog,
    context: &BindingContext,
) -> Result<(), CassieError> {
    let AlterTableOperation::AlterColumnType { using, .. } = operation else {
        return Ok(());
    };
    let local = super::local_name(table);
    let qualifiers: [&str; 3] = [source_table, table, &local];
    let mut fields = Vec::with_capacity(schema.fields.len() * 4);
    for field in &schema.fields {
        let key = crate::sql::ColumnIdentifierPath::from_field_name(&field.name).lookup_key();
        fields.push(FieldSchema {
            name: key.clone(),
            data_type: field.data_type.clone(),
            nullable: true,
        });
        for qualifier in qualifiers {
            fields.push(FieldSchema {
                name: format!("{qualifier}.{key}"),
                data_type: field.data_type.clone(),
                nullable: true,
            });
        }
    }
    let types = ResultTypes::for_source(
        &crate::sql::ast::QuerySource::SingleRow,
        &[],
        catalog,
        context,
    )?
    .with_extra_fields(fields)
    .with_resolved_coalesce_domains(true);
    boolean_contexts::validate_value(using, &types, None, catalog, context)
}
