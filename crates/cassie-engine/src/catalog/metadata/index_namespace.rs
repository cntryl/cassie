//! Logical relation namespace for index naming.
use super::Catalog;
use crate::app::CassieError;
use crate::catalog::{parse_name, ParsedName, ProjectionKind, DEFAULT_SCHEMA};
use crate::runtime::QueryExecutionControls;

#[path = "index_namespace_snapshot.rs"]
mod snapshot;

#[cfg(test)]
#[path = "index_namespace_tests.rs"]
mod tests;

enum Parts {
    Checked(crate::catalog::controlled_name::Parts),
    Unchecked(ParsedName),
}

impl Parts {
    fn parse(raw: &str, controls: Option<&QueryExecutionControls>) -> Result<Self, CassieError> {
        if let Some(controls) = controls {
            let parts = crate::catalog::controlled_name::parse_parts(raw, controls)?
                .ok_or_else(|| CassieError::Planner("invalid catalog relation name".to_owned()))?;
            if parts.values.len() > 3 {
                return Err(CassieError::Planner(
                    "invalid catalog relation name".to_owned(),
                ));
            }
            Ok(Self::Checked(parts))
        } else {
            Ok(Self::Unchecked(
                parse_name(raw).map_err(CassieError::Planner)?,
            ))
        }
    }

    fn components<'a>(&'a self, database: &'a str) -> (&'a str, &'a str, &'a str) {
        match self {
            Self::Checked(parts) => match parts.values.as_slice() {
                [name] => (database, DEFAULT_SCHEMA, name),
                [schema, name] => (database, schema, name),
                [database, schema, name] => (database, schema, name),
                _ => unreachable!("validated catalog identifier components"),
            },
            Self::Unchecked(ParsedName::Unqualified(name)) => (database, DEFAULT_SCHEMA, name),
            Self::Unchecked(ParsedName::SchemaQualified { schema, name }) => {
                (database, schema, name)
            }
            Self::Unchecked(ParsedName::DatabaseQualified {
                database,
                schema,
                name,
            }) => (database, schema, name),
        }
    }
}

fn equal(
    left: &str,
    right: &str,
    fold: bool,
    controls: Option<&QueryExecutionControls>,
) -> Result<bool, CassieError> {
    if left.len() != right.len() {
        return Ok(false);
    }
    for (left, right) in left
        .as_bytes()
        .chunks(4096)
        .zip(right.as_bytes().chunks(4096))
    {
        if let Some(controls) = controls {
            controls.check_interruption()?;
        }
        if if fold {
            !left.eq_ignore_ascii_case(right)
        } else {
            left != right
        } {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Reports whether a candidate relation name matches the target exactly, or
/// only after ASCII case folding.
fn name_match(
    candidate: (&str, &str, &str),
    target: (&str, &str, &str),
    controls: Option<&QueryExecutionControls>,
) -> Result<(bool, bool), CassieError> {
    let matches = |fold| -> Result<bool, CassieError> {
        Ok(equal(candidate.0, target.0, fold, controls)?
            && equal(candidate.1, target.1, fold, controls)?
            && equal(candidate.2, target.2, fold, controls)?)
    };
    if matches(false)? {
        return Ok((true, false));
    }
    Ok((false, matches(true)?))
}

fn read<'a, T>(
    lock: &'a parking_lot::RwLock<T>,
    controls: Option<&QueryExecutionControls>,
    after_wait: &mut impl FnMut(),
    after_acquire: &mut impl FnMut(),
) -> Result<parking_lot::RwLockReadGuard<'a, T>, CassieError> {
    let Some(controls) = controls else {
        return Ok(lock.read());
    };
    loop {
        controls.check_interruption()?;
        if let Some(guard) = lock.try_read_for(std::time::Duration::from_millis(2)) {
            after_acquire();
            controls.check_interruption()?;
            return Ok(guard);
        }
        after_wait();
    }
}

impl Catalog {
    pub(crate) fn index_relation_name_occupied(
        &self,
        name: &str,
        database: &str,
        generated: bool,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<bool, CassieError> {
        self.index_relation_name_occupied_with(name, database, generated, controls, || {}, || {})
    }

    fn index_relation_name_occupied_with(
        &self,
        name: &str,
        database: &str,
        generated: bool,
        controls: Option<&QueryExecutionControls>,
        mut after_collection: impl FnMut(),
        mut after_wait: impl FnMut(),
    ) -> Result<bool, CassieError> {
        if let Some(controls) = controls {
            controls.check_interruption()?;
        }
        let target = Parts::parse(name, controls)?;
        let (target_database, target_schema, target_name) = target.components(database);
        let mut exact = false;
        let mut ambiguous = false;
        let target_parts = (target_database, target_schema, target_name);
        let mut record = |db: &str, schema: &str, name: &str| -> Result<(), CassieError> {
            let (is_exact, is_folded) = name_match((db, schema, name), target_parts, controls)?;
            exact |= is_exact;
            ambiguous |= is_folded;
            Ok(())
        };
        // Copy only names under their own guards, then examine projections with no
        // collection or index guard held. Direct Catalog mutation fencing remains pending.
        let mut collections = snapshot::Names::new(controls)?;
        let mut after_acquire = || {};
        let collection_guard = read(
            &self.collections,
            controls,
            &mut after_wait,
            &mut after_acquire,
        )?;
        for collection in collection_guard.values() {
            collections.push_name(&collection.name, controls)?;
            after_collection();
        }
        drop(collection_guard);
        let mut indexes = snapshot::Names::new(controls)?;
        let index_guard = read(&self.indexes, controls, &mut after_wait, &mut after_acquire)?;
        for index in index_guard.values() {
            indexes.push_index(&index.collection, &index.name, controls)?;
        }
        drop(index_guard);
        let projections = read(
            &self.projections,
            controls,
            &mut after_wait,
            &mut after_acquire,
        )?;
        for collection in &collections.values {
            let candidate = Parts::parse(collection, controls)?;
            if projection_for_output(projections.values(), collection, controls)?.is_none() {
                let (db, schema, name) = candidate.components(target_database);
                record(db, schema, name)?;
            }
        }
        for projection in projections.values() {
            if let Some(controls) = controls {
                controls.check_interruption()?;
            }
            if projection.kind == ProjectionKind::Materialized {
                let candidate = Parts::parse(&projection.collection, controls)?;
                let (db, schema, name) = candidate.components(target_database);
                record(db, schema, name)?;
            }
        }
        for index in &indexes.values {
            let projection =
                projection_for_output(projections.values(), &index.collection, controls)?;
            let Some(owner_name) = index_owner_name(projection, &index.collection, controls)?
            else {
                continue;
            };
            let owner = Parts::parse(owner_name, controls)?;
            let index_name = Parts::parse(&index.name, controls)?;
            let (db, schema, _) = owner.components(target_database);
            let (_, _, name) = index_name.components(db);
            record(db, schema, name)?;
        }
        drop(projections);
        let views = read(&self.views, controls, &mut after_wait, &mut after_acquire)?;
        for view in views.values() {
            let candidate = Parts::parse(&view.name, controls)?;
            let (db, schema, name) = candidate.components(target_database);
            record(db, schema, name)?;
        }
        drop(views);
        let sequences = read(
            &self.sequences,
            controls,
            &mut after_wait,
            &mut after_acquire,
        )?;
        for sequence in sequences.values() {
            let candidate = Parts::parse(&sequence.name, controls)?;
            let (db, schema, name) = candidate.components(target_database);
            record(db, schema, name)?;
        }
        if exact || (ambiguous && generated) {
            return Ok(true);
        }
        if ambiguous {
            return Err(CassieError::Unsupported(
                "case-distinct relation names cannot yet be given independent index identities"
                    .to_owned(),
            ));
        }
        Ok(false)
    }
}

/// Resolves the relation that owns an index name, or `None` when the index
/// belongs to an inactive materialized-view version.
fn index_owner_name<'a>(
    projection: Option<&'a crate::catalog::ProjectionMeta>,
    index_collection: &'a str,
    controls: Option<&QueryExecutionControls>,
) -> Result<Option<&'a str>, CassieError> {
    let Some(projection) = projection else {
        return Ok(Some(index_collection));
    };
    if !projection
        .materialized
        .as_ref()
        .is_some_and(crate::catalog::MaterializedProjectionMeta::is_sql_materialized_view)
    {
        return Ok(Some(index_collection));
    }
    let mut active = None;
    for version in &projection.versions {
        if let Some(controls) = controls {
            controls.check_interruption()?;
        }
        if let Some(id) = projection.active_version.as_ref() {
            if equal(id, &version.version_id, false, controls)? {
                active = Some(version.output_collection.as_str());
                break;
            }
        }
    }
    match active {
        Some(active) if equal(active, index_collection, false, controls)? => {
            Ok(Some(projection.collection.as_str()))
        }
        _ => Ok(None),
    }
}

fn projection_for_output<'a>(
    projections: impl Iterator<Item = &'a crate::catalog::ProjectionMeta>,
    output: &str,
    controls: Option<&QueryExecutionControls>,
) -> Result<Option<&'a crate::catalog::ProjectionMeta>, CassieError> {
    let mut found = None;
    for projection in projections {
        if let Some(controls) = controls {
            controls.check_interruption()?;
        }
        if projection.kind != ProjectionKind::Materialized {
            continue;
        }
        for version in &projection.versions {
            if let Some(controls) = controls {
                controls.check_interruption()?;
            }
            if equal(&version.output_collection, output, false, controls)? {
                if found.is_some() {
                    return Err(CassieError::Planner(
                        "ambiguous materialized projection output".to_owned(),
                    ));
                }
                found = Some(projection);
                break;
            }
        }
    }
    Ok(found)
}
