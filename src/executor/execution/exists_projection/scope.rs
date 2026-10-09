//! Static enclosing names copied only after admitted source metadata.
use super::admission;
use super::{check_timeout, ExistsResolutionContext, HashSet, QueryError, QuerySource};
use crate::executor::retained_memory::{add, hash_table_bytes, mul};
use crate::runtime::accounted::AccountedVec;
use crate::runtime::QueryMemoryReservation;

pub(in crate::executor::execution) struct Scope {
    pub(super) fields: HashSet<String>,
    _memory: QueryMemoryReservation,
}

impl Scope {
    pub(super) fn new(
        context: &ExistsResolutionContext<'_>,
        source: &QuerySource,
    ) -> Result<Self, QueryError> {
        check_timeout(context.controls)?;
        let schemas = schemas(context, source)?;
        let namespace = super::super::cte::context_fields(context.cte_context, context.controls)?;
        let mut memory = admission::reserve_clone(&(source, &*namespace), context.controls)?;
        let schema_bytes = schemas
            .as_slice()
            .iter()
            .try_fold(0_usize, |bytes, (_, owner)| {
                bytes
                    .checked_add(owner.as_ref().map_or(0, QueryMemoryReservation::bytes))
                    .ok_or_else(|| {
                        crate::app::CassieError::ResourceLimit(
                            "EXISTS source metadata size overflow".into(),
                        )
                    })
            })?;
        memory.try_grow(schema_bytes.checked_mul(256).ok_or_else(|| {
            crate::app::CassieError::ResourceLimit("EXISTS source copy size overflow".into())
        })?)?;
        let mut names = HashSet::new();
        collect_names(context, source, &namespace, &mut names, &mut memory)?;
        Ok(Self {
            fields: names,
            _memory: memory,
        })
    }
}

fn collect_names(
    context: &ExistsResolutionContext<'_>,
    source: &QuerySource,
    namespace: &std::collections::HashMap<String, Vec<crate::types::FieldSchema>>,
    names: &mut HashSet<String>,
    memory: &mut QueryMemoryReservation,
) -> Result<(), QueryError> {
    check_timeout(context.controls)?;
    if let QuerySource::Join { left, right, .. } = source {
        collect_names(context, left, namespace, names, memory)?;
        return collect_names(context, right, namespace, names, memory);
    }
    let fields = crate::sql::binder::source_row_fields(
        source,
        namespace,
        &context.cassie.catalog,
        context.user_functions,
    )
    .map_err(|error| QueryError::General(error.to_string()))?;
    let qualifier = super::super::exists_correlated::outer_qualifier(source);
    let qualifiers = if let QuerySource::Collection(name) = source {
        crate::catalog::qualifier_variants(name)
    } else {
        qualifier.into_iter().collect()
    };
    let variants = qualifiers.len().max(1);
    let capacity = fields
        .len()
        .checked_mul(variants)
        .and_then(|count| count.checked_mul(2))
        .ok_or_else(|| {
            crate::app::CassieError::ResourceLimit("EXISTS source field count overflow".into())
        })?;
    let longest_qualifier = qualifiers.iter().map(String::len).max().unwrap_or(0);
    let names_bytes = fields.iter().try_fold(0, |bytes, field| {
        check_timeout(context.controls)?;
        // Cover quoted literal rendering, both canonical keys and temporary copies.
        let rendered = add(add(longest_qualifier, 3)?, mul(field.name.len(), 2)?)?;
        add(bytes, mul(variants, add(mul(rendered, 8)?, 8)?)?)
    })?;
    // Keep old table backing charged while reserving its complete replacement.
    // Retaining the conservative table estimate also covers later leaf rehashes.
    memory.try_grow(add(
        names_bytes,
        hash_table_bytes::<String>(add(names.len(), capacity)?)?,
    )?)?;
    names
        .try_reserve(capacity)
        .map_err(|error| crate::app::CassieError::ResourceLimit(error.to_string()))?;
    for field in fields {
        check_timeout(context.controls)?;
        if qualifiers.is_empty() {
            insert_name(names, &field.name);
        } else {
            for qualifier in &qualifiers {
                check_timeout(context.controls)?;
                let name = super::super::outer_names::outer_field_name(
                    &context.cassie.catalog,
                    source,
                    Some(qualifier),
                    &field.name,
                );
                insert_name(names, &name);
            }
        }
    }
    Ok(())
}

fn insert_name(names: &mut HashSet<String>, name: &str) {
    names.insert(crate::sql::ColumnIdentifierPath::stored_row_lookup_key(
        name,
    ));
    names.insert(crate::sql::ColumnIdentifierPath::stored_row_field_key(name));
}

type Schemas = AccountedVec<(
    Option<crate::catalog::CollectionSchema>,
    Option<QueryMemoryReservation>,
)>;

fn schemas(
    context: &ExistsResolutionContext<'_>,
    source: &QuerySource,
) -> Result<Schemas, QueryError> {
    fn collect(
        context: &ExistsResolutionContext<'_>,
        source: &QuerySource,
        schemas: &mut Schemas,
    ) -> Result<(), QueryError> {
        check_timeout(context.controls)?;
        match source {
            QuerySource::Collection(name) => schemas.try_push_with_result(0, || {
                context
                    .cassie
                    .catalog
                    .clone_schema_with_controls(name, context.controls)
            })?,
            QuerySource::Aliased { source, .. } => collect(context, source, schemas)?,
            QuerySource::Join { left, right, .. } => {
                collect(context, left, schemas)?;
                collect(context, right, schemas)?;
            }
            QuerySource::Subquery { select, .. } => collect(context, &select.source, schemas)?,
            _ => {}
        }
        Ok(())
    }
    let mut schemas = AccountedVec::try_new(context.controls)?;
    collect(context, source, &mut schemas)?;
    Ok(schemas)
}

#[cfg(test)]
#[path = "scope_tests.rs"]
mod tests;
