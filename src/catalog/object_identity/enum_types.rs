//! `regtype` input and output for user enum types and their `name[]` array
//! types (#827).
use super::{quote_identifier, ObjectScope};
use crate::catalog::canonical_relation_name;

impl ObjectScope<'_> {
    /// A user enum type named by `text`, or its `[]` array type.
    pub(super) fn resolve_enum_type_oid(&self, text: &str) -> Option<u32> {
        let search_order = self.context.relation_search_order();
        let resolve = |name: &str| {
            self.catalog
                .resolve_enum_type(&self.context.database, &search_order, name)
        };
        match text.trim_end().strip_suffix("[]") {
            Some(base) => resolve(base.trim_end()).map(|metadata| metadata.array_oid()),
            None => resolve(text).map(|metadata| metadata.type_oid()),
        }
    }

    /// `regtype` output (`format_type`) for a user enum type or its array
    /// type: qualified only when the search path would not find the same type.
    pub(super) fn enum_type_label(&self, oid: u32) -> Option<String> {
        let (metadata, suffix) = self.catalog.get_enum_type_by_oid(oid).map_or_else(
            || {
                self.catalog
                    .get_enum_type_by_array_oid(oid)
                    .map(|metadata| (metadata, "[]"))
            },
            |metadata| Some((metadata, "")),
        )?;
        let search_order = self.context.relation_search_order();
        let local = metadata.local_name();
        // Like TypeIsVisible, the first schema on the search path holding any
        // type of this name decides.
        let visible = search_order
            .iter()
            .find(|schema| {
                let canonical = canonical_relation_name(&self.context.database, schema, &local);
                self.catalog.enum_type_exists(&canonical)
                    || self.catalog.domain_type_exists(&canonical)
                    || self.catalog.composite_type_exists(&canonical)
                    || self.catalog.relation_row_type(&canonical).is_some()
            })
            .is_some_and(|schema| schema.eq_ignore_ascii_case(&metadata.schema_name()));
        let name = if visible {
            quote_identifier(&local)
        } else {
            format!(
                "{}.{}",
                quote_identifier(&metadata.schema_name()),
                quote_identifier(&local)
            )
        };
        Some(format!("{name}{suffix}"))
    }
}

/// The `pg_type.oid` of an enum column type (its array type for an enum array
/// column), or `None` for any other type.
pub(crate) fn enum_column_type_oid(data_type: &crate::types::DataType) -> Option<i64> {
    use crate::types::DataType;
    match data_type {
        DataType::Enum(descriptor) => Some(i64::from(descriptor.oid)),
        DataType::Array(inner) => match inner.as_ref() {
            DataType::Enum(descriptor) => Some(i64::from(descriptor.array_oid)),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
#[path = "enum_types_tests.rs"]
mod tests;
