//! `regtype` input and output for user-defined types: enum, composite, domain
//! and table row types, and their `name[]` array types (#818, #827, #828).
//!
//! A type name is unique within a schema across those families, so one
//! search-path walk decides which type an unqualified name denotes and whether
//! output needs its schema, as PostgreSQL's `TypenameGetTypid` and
//! `TypeIsVisible` do.
use super::names::{qualified_name, strip_array_dimensions, Identifier};
use super::{quote_identifier, ObjectScope};
use crate::catalog::{canonical_identifier_component, UserType};
use crate::types::ObjectIdentifierError;

impl ObjectScope<'_> {
    /// A user type named by `text`. An array type is spelled with any number
    /// of `[]` or `[n]` dimensions, or by its `pg_type.typname` (`_name`).
    pub(super) fn resolve_user_type(
        &self,
        text: &str,
    ) -> Result<Option<u32>, ObjectIdentifierError> {
        let (base, is_array) = strip_array_dimensions(text)?;
        let Ok(parts) = qualified_name(base) else {
            return Ok(None);
        };
        let (qualifier, name) = match parts.as_slice() {
            [name] => (None, name),
            [schema, name] => (Some(schema), name),
            [database, schema, name] => {
                if !database.value.eq_ignore_ascii_case(&self.context.database) {
                    return Err(ObjectIdentifierError::CrossDatabase(text.to_string()));
                }
                (Some(schema), name)
            }
            _ => {
                return Err(ObjectIdentifierError::ImproperQualifiedName(
                    text.to_string(),
                ))
            }
        };
        if let Some(found) = self.named_user_type(qualifier, name) {
            return Ok(Some(if is_array {
                found.array_oid()
            } else {
                found.type_oid()
            }));
        }
        // `_name` is the catalog name of the array type of `name`.
        let element = name
            .value
            .strip_prefix('_')
            .filter(|element| !is_array && !element.is_empty());
        Ok(element.and_then(|element| {
            let element = Identifier {
                value: element.to_string(),
                quoted: name.quoted,
            };
            self.named_user_type(qualifier, &element)
                .map(|found| found.array_oid())
        }))
    }

    /// The user type `name` denotes, in `qualifier` or through the search path.
    fn named_user_type(
        &self,
        qualifier: Option<&Identifier>,
        name: &Identifier,
    ) -> Option<UserType> {
        let search_order = self.context.relation_search_order();
        let raw = qualifier
            .into_iter()
            .chain(std::iter::once(name))
            .map(|part| canonical_identifier_component(&part.value))
            .collect::<Vec<_>>()
            .join(".");
        self.catalog
            .resolve_user_type(&self.context.database, &search_order, &raw)
            .filter(|found| Self::spelled_as(found, name))
    }

    /// Whether `found` is stored under the spelling `name` asks for. DDL folds
    /// an unquoted name to lower case and keeps a quoted one, so the stored
    /// name of a type or table row type matches exactly.
    fn spelled_as(found: &UserType, name: &Identifier) -> bool {
        found.local_name() == name.value
    }

    /// `regtype` output (`format_type`) for a user type or its array type:
    /// the name, schema-qualified unless the search path finds that type.
    pub(super) fn user_type_label(&self, oid: u32) -> Option<String> {
        let (schema, local, is_array) = self.user_type_parts(oid)?;
        let name = if self.user_type_is_visible(&schema, &local) {
            quote_identifier(&local)
        } else {
            format!("{}.{}", quote_identifier(&schema), quote_identifier(&local))
        };
        Some(format!("{name}{}", if is_array { "[]" } else { "" }))
    }

    /// `TypeIsVisible`: the first schema on the search path holding any user
    /// type called `local` is `schema`.
    fn user_type_is_visible(&self, schema: &str, local: &str) -> bool {
        let search_order = self.context.relation_search_order();
        self.catalog
            .first_user_type_on_path(&self.context.database, &search_order, local)
            .map_or_else(
                // An instance without a database catalog stores tables under
                // unscoped names the canonical walk does not see.
                || {
                    self.catalog.list_databases().is_empty()
                        && search_order
                            .iter()
                            .any(|candidate| candidate.eq_ignore_ascii_case(schema))
                },
                |(first, _)| first.eq_ignore_ascii_case(schema),
            )
    }
}

#[cfg(test)]
#[path = "user_types_tests.rs"]
mod tests;
