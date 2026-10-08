//! Static enclosing names copied only after admitted source metadata.
use super::admission;
use super::{check_timeout, ExistsResolutionContext, HashSet, QueryError, QuerySource};
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
        let fields = crate::sql::binder::source_row_fields(
            source,
            &namespace,
            &context.cassie.catalog,
            context.user_functions,
        )
        .map_err(|error| QueryError::General(error.to_string()))?;
        let qualifier = super::super::exists_correlated::outer_qualifier(source);
        let mut names = HashSet::new();
        names
            .try_reserve(fields.len().saturating_mul(2))
            .map_err(|error| crate::app::CassieError::ResourceLimit(error.to_string()))?;
        for field in fields {
            check_timeout(context.controls)?;
            let name = if field.name.contains('.') {
                field.name
            } else if let Some(qualifier) = &qualifier {
                format!("{qualifier}.{}", field.name)
            } else {
                field.name
            };
            names.insert(crate::sql::ColumnIdentifierPath::stored_row_lookup_key(
                &name,
            ));
            names.insert(crate::sql::ColumnIdentifierPath::stored_row_field_key(
                &name,
            ));
        }
        Ok(Self {
            fields: names,
            _memory: memory,
        })
    }
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
