//! Catalog-scoped parsing for re-parses that run during binding or planning.
//!
//! The SQL parser is context free: a composite, row-type or domain name only
//! parses while a type scope and a domain scope are installed. Session entry
//! points install them in `Cassie::parse_statement_for_session`; code that
//! re-parses stored or nested SQL text (view bodies, projection queries,
//! dependency scans) uses [`parse_statement_in_catalog`] so those names keep
//! resolving against the catalog and search path the caller already holds.

use std::rc::Rc;

use super::binder::BindingContext;
use super::parser::{parse_statement, with_domain_scope, with_enum_scope, with_type_scope};
use super::{DomainResolver, EnumResolver, SqlError, TypeResolver};
use crate::catalog::Catalog;
use crate::sql::ast::ParsedStatement;
use crate::types::{CompositeRef, DomainRef, EnumRef};

fn search_order(context: &BindingContext) -> Vec<String> {
    context
        .relation_search_order()
        .into_iter()
        .map(str::to_owned)
        .collect()
}

fn type_resolver(catalog: &Catalog, context: &BindingContext) -> TypeResolver {
    let catalog = catalog.clone();
    let database = context.database.clone();
    let order = search_order(context);
    Rc::new(move |raw: &str| -> Option<CompositeRef> {
        let order = order.iter().map(String::as_str).collect::<Vec<_>>();
        catalog
            .resolve_composite_type(&database, &order, raw)
            .map(|metadata| metadata.descriptor())
            .or_else(|| catalog.resolve_relation_row_type(&database, &order, raw))
    })
}

fn domain_resolver(catalog: &Catalog, context: &BindingContext) -> DomainResolver {
    let catalog = catalog.clone();
    let database = context.database.clone();
    let order = search_order(context);
    Rc::new(move |raw: &str| -> Option<DomainRef> {
        let order = order.iter().map(String::as_str).collect::<Vec<_>>();
        catalog
            .resolve_domain_type(&database, &order, raw)
            .map(|metadata| metadata.descriptor())
    })
}

fn enum_resolver(catalog: &Catalog, context: &BindingContext) -> EnumResolver {
    let catalog = catalog.clone();
    let database = context.database.clone();
    let order = search_order(context);
    Rc::new(move |raw: &str| -> Option<EnumRef> {
        let order = order.iter().map(String::as_str).collect::<Vec<_>>();
        catalog
            .resolve_enum_type(&database, &order, raw)
            .map(|metadata| metadata.descriptor())
    })
}

/// Parses `sql` with the composite, row-type, domain and enum names visible in
/// `catalog` through `context`'s search path in scope.
///
/// # Errors
///
/// Returns the parser's [`SqlError`] when `sql` is not valid.
pub fn parse_statement_in_catalog(
    catalog: &Catalog,
    context: &BindingContext,
    sql: &str,
) -> Result<ParsedStatement, SqlError> {
    if !catalog.has_composite_types()
        && !catalog.has_collections()
        && !catalog.has_domain_types()
        && !catalog.has_enum_types()
    {
        return parse_statement(sql);
    }
    let domains = domain_resolver(catalog, context);
    let types = type_resolver(catalog, context);
    let enums = enum_resolver(catalog, context);
    with_enum_scope(enums, || {
        with_domain_scope(domains, || with_type_scope(types, || parse_statement(sql)))
    })
}
