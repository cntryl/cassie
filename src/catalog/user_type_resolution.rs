//! One search-path walk for every user-defined type family (#818, #827, #828).
//!
//! A type name is unique within a schema across enum, composite, domain and
//! table row types, so an unqualified name denotes whichever of them the
//! first schema on the search path holds, as PostgreSQL's `TypenameGetTypid`
//! does. Resolving each family with its own walk would instead let a
//! composite in a later schema shadow a domain or enum in an earlier one.
use super::{
    canonical_relation_name, local_name, parse_name, Catalog, CompositeTypeMeta, DomainTypeMeta,
    EnumTypeMeta, ParsedName,
};
use crate::types::composite::CompositeRef;
use crate::types::DataType;

/// The user-defined type a name denotes.
#[derive(Clone)]
pub enum UserType {
    Enum(EnumTypeMeta),
    Composite(CompositeTypeMeta),
    Domain(DomainTypeMeta),
    /// The row type every table owns.
    RowType(CompositeRef),
}

impl UserType {
    /// `pg_type.oid` of the type.
    #[must_use]
    pub fn type_oid(&self) -> u32 {
        match self {
            Self::Enum(metadata) => metadata.type_oid(),
            Self::Composite(metadata) => metadata.type_oid(),
            Self::Domain(metadata) => metadata.type_oid(),
            Self::RowType(descriptor) => descriptor.oid,
        }
    }

    /// `pg_type.oid` of the type's `_name` array type.
    #[must_use]
    pub fn array_oid(&self) -> u32 {
        match self {
            Self::Enum(metadata) => metadata.array_oid(),
            Self::Composite(metadata) => metadata.array_oid(),
            Self::Domain(metadata) => metadata.array_oid(),
            Self::RowType(descriptor) => descriptor.array_oid,
        }
    }

    /// The schema-local name the type is stored under.
    #[must_use]
    pub fn local_name(&self) -> String {
        match self {
            Self::Enum(metadata) => metadata.local_name(),
            Self::Composite(metadata) => metadata.local_name(),
            Self::Domain(metadata) => metadata.local_name(),
            Self::RowType(descriptor) => descriptor.name.clone(),
        }
    }
}

impl Catalog {
    /// The user-defined type named `name` in `schema` of `database`.
    #[must_use]
    pub fn user_type_in_schema(
        &self,
        database: &str,
        schema: &str,
        name: &str,
    ) -> Option<UserType> {
        let canonical = canonical_relation_name(database, schema, name);
        self.get_enum_type(&canonical)
            .map(UserType::Enum)
            .or_else(|| self.get_composite_type(&canonical).map(UserType::Composite))
            .or_else(|| self.get_domain_type(&canonical).map(UserType::Domain))
            .or_else(|| self.relation_row_type(&canonical).map(UserType::RowType))
    }

    /// The first schema of `search_order` that holds a user-defined type named
    /// `name`, with that type.
    #[must_use]
    pub fn first_user_type_on_path<'path>(
        &self,
        database: &str,
        search_order: &[&'path str],
        name: &str,
    ) -> Option<(&'path str, UserType)> {
        search_order.iter().find_map(|schema| {
            self.user_type_in_schema(database, schema, name)
                .map(|found| (*schema, found))
        })
    }

    /// Resolves a type name the way PostgreSQL resolves one: an unqualified
    /// name denotes the type in the first schema of `search_order` holding any
    /// user-defined type of that name; a qualified name is exact.
    #[must_use]
    pub fn resolve_user_type(
        &self,
        database: &str,
        search_order: &[&str],
        raw: &str,
    ) -> Option<UserType> {
        match parse_name(raw).ok()? {
            ParsedName::Unqualified(name) => self
                .first_user_type_on_path(database, search_order, &name)
                .map(|(_, found)| found)
                // An unscoped catalog stores tables under their bare names.
                .or_else(|| {
                    self.resolve_relation_row_type(database, &[], raw)
                        .map(UserType::RowType)
                }),
            ParsedName::SchemaQualified { schema, name } => self
                .user_type_in_schema(database, &schema, &name)
                .or_else(|| {
                    self.resolve_relation_row_type(database, &[], raw)
                        .map(UserType::RowType)
                }),
            ParsedName::DatabaseQualified {
                database,
                schema,
                name,
            } => self.user_type_in_schema(&database, &schema, &name),
        }
    }
}

/// Whether `data_type` is, contains, wraps or arrays the user type `oid`.
fn data_type_uses(data_type: &DataType, oid: u32) -> bool {
    match data_type {
        DataType::Composite(descriptor) => {
            descriptor.oid == oid
                || descriptor
                    .fields
                    .iter()
                    .any(|field| data_type_uses(&field.data_type, oid))
        }
        DataType::Domain(descriptor) => {
            descriptor.oid == oid || data_type_uses(&descriptor.base, oid)
        }
        DataType::Enum(descriptor) => descriptor.oid == oid,
        DataType::Array(inner) => data_type_uses(inner, oid),
        _ => false,
    }
}

impl Catalog {
    /// Functions and procedures whose arguments, or result, use the user type
    /// `oid`: each keeps the type alive (`2BP01` on `DROP`), as in PostgreSQL.
    /// Pairs the routine's stored name with the dependency detail phrase.
    #[must_use]
    pub fn routine_dependents_on_type(&self, oid: u32) -> Vec<(String, String)> {
        let functions = self.list_functions().into_iter().filter_map(|function| {
            (data_type_uses(&function.return_type, oid)
                || function
                    .args
                    .iter()
                    .any(|argument| data_type_uses(&argument.data_type, oid)))
            .then(|| {
                let phrase = format!("function {}", local_name(&function.name));
                (function.name, phrase)
            })
        });
        let procedures = self.list_procedures().into_iter().filter_map(|procedure| {
            procedure
                .args
                .iter()
                .any(|argument| data_type_uses(&argument.data_type, oid))
                .then(|| {
                    let phrase = format!("procedure {}", local_name(&procedure.name));
                    (procedure.name, phrase)
                })
        });
        functions.chain(procedures).collect()
    }
}

#[cfg(test)]
#[path = "user_type_resolution_tests.rs"]
mod tests;
