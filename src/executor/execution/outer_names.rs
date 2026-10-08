//! Literal emitted components use existing source and projection provenance.
use crate::catalog::Catalog;
use crate::sql::{ColumnIdentifierPath, QuerySource, SelectItem};

pub(super) fn outer_field_name(
    catalog: &Catalog,
    source: &QuerySource,
    qualifier: Option<&str>,
    name: &str,
) -> String {
    let Some(qualifier) = qualifier else {
        return name.to_owned();
    };
    let literal = literal_field(catalog, source, name);
    if literal || !name.contains('.') {
        let field = if literal {
            ColumnIdentifierPath::from_field_name(name).lookup_key()
        } else {
            name.to_owned()
        };
        format!("{qualifier}.{field}")
    } else {
        name.to_owned()
    }
}
fn literal_field(catalog: &Catalog, source: &QuerySource, name: &str) -> bool {
    match source {
        QuerySource::Aliased { source, .. } => literal_field(catalog, source, name),
        QuerySource::Collection(collection) => {
            catalog.contains_declared_stored_field(collection, name)
        }
        QuerySource::Subquery { select, .. } => select.projection.iter().any(|item| match item {
            SelectItem::Column {
                name: reference,
                alias,
            } => alias.as_deref().map_or_else(
                || {
                    ColumnIdentifierPath::parse(reference)
                        .is_ok_and(|path| !path.is_qualified() && path.final_name() == name)
                },
                |alias| alias == name,
            ),
            SelectItem::Expr { alias, .. }
            | SelectItem::Function { alias, .. }
            | SelectItem::WindowFunction { alias, .. } => alias.as_deref() == Some(name),
            SelectItem::Wildcard => declared_field(catalog, &select.source, name),
        }),
        _ => false,
    }
}

fn declared_field(catalog: &Catalog, source: &QuerySource, name: &str) -> bool {
    match source {
        QuerySource::Aliased { source, .. } => declared_field(catalog, source, name),
        QuerySource::Collection(collection) => {
            catalog.contains_declared_stored_field(collection, name)
        }
        _ => false,
    }
}
