//! Visible alias keys belong to expression lookup, not wildcard output shape.

use super::{FieldSchema, QuerySource, Schema};
use crate::sql::ColumnIdentifierPath;

pub(super) fn append_visible_fields(source: &QuerySource, lookup: &mut Schema) {
    let QuerySource::Aliased {
        source: inner,
        alias,
        column_aliases,
    } = source
    else {
        return;
    };
    let prefix = format!("{}.", crate::sql::binder::aliases::qualifier(alias));
    let visible_qualifier = ColumnIdentifierPath::parse(alias)
        .map_or_else(|_| alias.clone(), |path| path.namespace_key());
    // The source schema already retains its physical and private-qualified
    // fields. Select one physical field per position and add only lookup keys.
    let physical = lookup
        .fields
        .iter()
        .filter_map(|field| {
            let name = field.name.strip_prefix(&prefix)?;
            ColumnIdentifierPath::parse(name)
                .is_ok_and(|path| !path.is_qualified())
                .then(|| (name.to_string(), field.data_type.clone(), field.nullable))
        })
        .collect::<Vec<_>>();
    let mut visible_fields = Vec::with_capacity(physical.len() * 2);
    for (index, (name, data_type, nullable)) in physical.into_iter().enumerate() {
        let visible = column_aliases
            .get(index)
            .cloned()
            .unwrap_or_else(|| ColumnIdentifierPath::stored_field_key(&name));
        visible_fields.push(FieldSchema {
            name: visible.clone(),
            data_type: data_type.clone(),
            nullable,
        });
        visible_fields.push(FieldSchema {
            name: format!("{visible_qualifier}.{visible}"),
            data_type,
            nullable,
        });
    }
    if matches!(inner.as_ref(), QuerySource::Collection(_)) {
        // A single collection alias can lower to bare physical references.
        lookup.fields.extend(visible_fields);
    } else {
        // Derived aliases lower to private-qualified physical references.
        // Their visible unqualified keys must precede a colliding physical
        // name, as in VALUES (ARRAY[1], 'x') AS s(column2,column1).
        visible_fields.append(&mut lookup.fields);
        lookup.fields = visible_fields;
    }
}
