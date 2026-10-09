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
/// Add exact literal spelling only for an alias emitted by its owning source.
pub(super) fn literal_outer_alias(
    catalog: &Catalog,
    source: &QuerySource,
    name: &str,
    field: &str,
) -> Option<String> {
    if let QuerySource::Join { left, right, .. } = source {
        return literal_outer_alias(catalog, left, name, field)
            .or_else(|| literal_outer_alias(catalog, right, name, field));
    }
    if !literal_field(catalog, source, field) {
        return None;
    }
    let qualifiers = if let QuerySource::Collection(collection) = source {
        crate::catalog::qualifier_variants(collection)
    } else {
        super::exists_correlated::outer_qualifier(source)
            .into_iter()
            .collect()
    };
    qualifiers.into_iter().find_map(|qualifier| {
        let suffix = name.strip_prefix(&qualifier)?.strip_prefix('.')?;
        (suffix == field).then(|| outer_field_name(catalog, source, Some(&qualifier), field))
    })
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
