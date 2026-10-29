use super::schema_namespace_command::{
    object_in_schema, rewrite_relation_name_for_schema, RelationRenames,
};
use crate::app::Cassie;
use crate::catalog::RelationId;
use crate::executor::execution::QueryError;
use std::collections::HashSet;

pub(super) struct OwnedTableSequenceMovePlan {
    updates: Vec<(String, crate::catalog::SequenceMeta)>,
    renames: RelationRenames,
}

struct SequenceMoveBuilder<'a> {
    cassie: &'a Cassie,
    current_table: &'a str,
    next_table: &'a str,
    next_relation: &'a RelationId,
    sequences: &'a [crate::catalog::SequenceMeta],
    indexes: &'a [crate::catalog::IndexMeta],
    reserved_names: HashSet<String>,
    moved_sources: HashSet<String>,
    plan: OwnedTableSequenceMovePlan,
}

impl SequenceMoveBuilder<'_> {
    fn prepare_explicit_owners(&mut self) -> Result<(), QueryError> {
        for sequence in self.sequences {
            let Some(owner) = sequence.owned_by.as_ref() else {
                continue;
            };
            if self.cassie.midge.canonical_collection_name(&owner.table) != self.current_table {
                continue;
            }

            let current_name = self.cassie.midge.canonical_collection_name(&sequence.name);
            let current_relation = RelationId::parse_canonical(&current_name).ok_or_else(|| {
                QueryError::General(format!(
                    "sequence '{current_name}' has no canonical database identity"
                ))
            })?;
            let next_name = RelationId::new(
                self.next_relation.database.clone(),
                self.next_relation.schema.clone(),
                current_relation.name,
            )
            .canonical_name();
            self.add_sequence(current_name, next_name, sequence.clone())?;
        }
        Ok(())
    }

    fn prepare_constraint_owners(&mut self) -> Result<(), QueryError> {
        for constraint in self
            .cassie
            .catalog
            .get_constraints(self.current_table)
            .into_iter()
            .filter(|constraint| constraint.default_sequence_owned.is_owned())
        {
            let Some(sequence_name) = constraint.default_sequence.as_deref() else {
                continue;
            };
            let current_name = self.cassie.midge.canonical_collection_name(sequence_name);
            if self
                .moved_sources
                .contains(&current_name.to_ascii_lowercase())
            {
                continue;
            }
            let Some(sequence) = self.sequences.iter().find_map(|sequence| {
                self.cassie
                    .midge
                    .canonical_collection_name(&sequence.name)
                    .eq_ignore_ascii_case(&current_name)
                    .then(|| sequence.clone())
            }) else {
                return Err(QueryError::General(format!(
                    "sequence '{current_name}' does not exist"
                )));
            };
            let next_name =
                self.cassie
                    .midge
                    .canonical_collection_name(&crate::catalog::serial_sequence_name(
                        self.next_table,
                        &constraint.field,
                    ));
            self.add_sequence(current_name, next_name, sequence)?;
        }
        Ok(())
    }

    fn add_sequence(
        &mut self,
        current_name: String,
        next_name: String,
        mut sequence: crate::catalog::SequenceMeta,
    ) -> Result<(), QueryError> {
        reserve_sequence_move_target(
            self.cassie,
            &current_name,
            &next_name,
            self.sequences,
            self.indexes,
            &mut self.reserved_names,
        )?;
        sequence.name.clone_from(&next_name);
        if let Some(owner) = sequence.owned_by.as_mut() {
            self.next_table.clone_into(&mut owner.table);
        }
        if !current_name.eq_ignore_ascii_case(&next_name) {
            self.plan
                .renames
                .insert(current_name.to_ascii_lowercase(), next_name);
        }
        self.moved_sources.insert(current_name.to_ascii_lowercase());
        self.plan.updates.push((current_name, sequence));
        Ok(())
    }

    fn finish(self) -> OwnedTableSequenceMovePlan {
        self.plan
    }
}

/// Releases sequences owned by a table when that table is dropped.
pub(super) fn drop_owned_table_sequences(cassie: &Cassie, table: &str) -> Result<(), QueryError> {
    for constraint in cassie
        .catalog
        .get_constraints(table)
        .into_iter()
        .filter(|constraint| constraint.default_sequence_owned.is_owned())
    {
        if let Some(sequence) = constraint.default_sequence.as_deref() {
            cassie.midge.delete_sequence(sequence)?;
            cassie.catalog.unregister_sequence(sequence);
        }
    }
    for sequence in cassie
        .catalog
        .list_sequences()
        .into_iter()
        .filter(|sequence| {
            sequence
                .owned_by
                .as_ref()
                .is_some_and(|owner| crate::catalog::name_matches(&owner.table, table))
        })
    {
        cassie.midge.delete_sequence(&sequence.name)?;
        cassie.catalog.unregister_sequence(&sequence.name);
    }
    Ok(())
}

/// Keeps SERIAL-owned sequence names aligned with their table after a rename.
pub(super) fn rename_owned_table_sequences(cassie: &Cassie, table: &str) -> Result<(), QueryError> {
    let mut constraints = cassie.catalog.get_constraints(table);
    let mut renames = Vec::new();
    let sequences = cassie.catalog.list_sequences();
    for (index, constraint) in constraints.iter().enumerate() {
        let Some(current_name) = constraint
            .default_sequence
            .as_deref()
            .filter(|_| constraint.default_sequence_owned.is_owned())
        else {
            continue;
        };
        let next_name = crate::catalog::serial_sequence_name(table, &constraint.field);
        if current_name.eq_ignore_ascii_case(&next_name) {
            continue;
        }
        if sequences
            .iter()
            .any(|sequence| sequence.name.eq_ignore_ascii_case(&next_name))
        {
            return Err(QueryError::General(format!(
                "sequence '{next_name}' already exists"
            )));
        }
        let sequence = cassie.midge.get_sequence(current_name)?.ok_or_else(|| {
            QueryError::General(format!("sequence '{current_name}' does not exist"))
        })?;
        renames.push((index, next_name, sequence));
    }

    for (index, next_name, mut sequence) in renames {
        let stored_name = sequence.name.clone();
        sequence.name = cassie.midge.canonical_collection_name(&next_name);
        if let Some(owner) = sequence.owned_by.as_mut() {
            owner.table = table.to_string();
        }
        sequence = super::super::sequence_command::canonicalize_sequence_metadata(cassie, sequence);
        cassie.midge.put_sequence(&sequence)?;
        cassie.midge.delete_sequence(&stored_name)?;
        cassie.catalog.unregister_sequence(&stored_name);
        cassie.catalog.register_sequence(sequence);
        constraints[index].default_sequence = Some(next_name.clone());
        constraints[index].default_expression =
            Some(crate::catalog::canonical_nextval_expression(&next_name));
    }

    if constraints.iter().any(|constraint| {
        constraint.default_sequence_owned.is_owned()
            && constraint
                .default_sequence
                .as_deref()
                .is_some_and(|sequence| {
                    crate::catalog::serial_sequence_name(table, &constraint.field)
                        .eq_ignore_ascii_case(sequence)
                })
    }) {
        cassie.midge.save_constraints(table, &constraints)?;
        cassie.catalog.register_constraints(table, constraints);
    }
    Ok(())
}

/// Validates and prepares column-owned sequences for a table schema move.
pub(super) fn prepare_owned_table_sequence_move(
    cassie: &Cassie,
    current_table: &str,
    next_table: &str,
) -> Result<OwnedTableSequenceMovePlan, QueryError> {
    let current_table = cassie.midge.canonical_collection_name(current_table);
    let next_table = cassie.midge.canonical_collection_name(next_table);
    let next_relation = RelationId::parse_canonical(&next_table).ok_or_else(|| {
        QueryError::General(format!(
            "relation '{next_table}' has no canonical database identity"
        ))
    })?;
    let sequences = cassie.catalog.list_sequences();
    let indexes = cassie.catalog.all_indexes_snapshot();
    let mut builder = SequenceMoveBuilder {
        cassie,
        current_table: &current_table,
        next_table: &next_table,
        next_relation: &next_relation,
        sequences: &sequences,
        indexes: &indexes,
        reserved_names: HashSet::new(),
        moved_sources: HashSet::new(),
        plan: OwnedTableSequenceMovePlan {
            updates: Vec::new(),
            renames: RelationRenames::new(),
        },
    };
    builder.prepare_explicit_owners()?;
    builder.prepare_constraint_owners()?;
    Ok(builder.finish())
}

fn reserve_sequence_move_target(
    cassie: &Cassie,
    current_name: &str,
    next_name: &str,
    sequences: &[crate::catalog::SequenceMeta],
    indexes: &[crate::catalog::IndexMeta],
    reserved_names: &mut HashSet<String>,
) -> Result<(), QueryError> {
    if !reserved_names.insert(next_name.to_ascii_lowercase()) {
        return Err(QueryError::General(format!(
            "sequence '{next_name}' already exists"
        )));
    }
    if !current_name.eq_ignore_ascii_case(next_name)
        && (sequences
            .iter()
            .any(|sequence| sequence.name.eq_ignore_ascii_case(next_name))
            || cassie.catalog.relation_exists(next_name)
            || indexes.iter().any(|index| {
                cassie
                    .midge
                    .canonical_collection_name(&index.name)
                    .eq_ignore_ascii_case(next_name)
            }))
    {
        return Err(QueryError::General(format!(
            "sequence '{next_name}' already exists"
        )));
    }
    Ok(())
}

/// Publishes a previously validated table-owned sequence schema move.
pub(super) fn move_owned_table_sequences(
    cassie: &Cassie,
    plan: OwnedTableSequenceMovePlan,
) -> Result<(), QueryError> {
    for (current_name, sequence) in plan.updates {
        cassie.midge.put_sequence(&sequence)?;
        if !current_name.eq_ignore_ascii_case(&sequence.name) {
            cassie.midge.delete_sequence(&current_name)?;
            cassie.catalog.unregister_sequence(&current_name);
        }
        cassie.catalog.register_sequence(sequence);
    }

    rewrite_constraint_references(cassie, &plan.renames, &RelationRenames::new())
}

/// Renames the sequences in a renamed schema and rewrites every column
/// constraint that still names a renamed sequence or relation.
pub(super) fn rename_schema_sequences(
    cassie: &Cassie,
    current_schema: &str,
    next_schema: &str,
    relation_renames: &RelationRenames,
) -> Result<(), QueryError> {
    let mut sequence_renames = RelationRenames::new();
    for mut sequence in cassie
        .catalog
        .list_sequences()
        .into_iter()
        .filter(|sequence| object_in_schema(&sequence.name, current_schema))
    {
        let current_name = sequence.name.clone();
        sequence.name =
            rewrite_relation_name_for_schema(&sequence.name, current_schema, next_schema);
        if sequence.name == current_name {
            continue;
        }
        cassie.midge.delete_sequence(&current_name)?;
        cassie.midge.put_sequence(&sequence)?;
        sequence_renames.insert(current_name.to_ascii_lowercase(), sequence.name);
    }
    rewrite_constraint_references(cassie, &sequence_renames, relation_renames)
}

fn rewrite_constraint_references(
    cassie: &Cassie,
    sequence_renames: &RelationRenames,
    relation_renames: &RelationRenames,
) -> Result<(), QueryError> {
    if sequence_renames.is_empty() && relation_renames.is_empty() {
        return Ok(());
    }
    for collection in cassie.catalog.list_collections_canonical() {
        let stored_name = relation_renames
            .get(&collection.name)
            .cloned()
            .unwrap_or(collection.name);
        let mut constraints = cassie.midge.load_constraints(&stored_name)?;
        let mut changed = false;
        for constraint in &mut constraints {
            changed |= rewrite_default_sequence(constraint, sequence_renames);
            if let Some(next) = constraint
                .references_table
                .as_ref()
                .and_then(|table| relation_renames.get(table))
            {
                constraint.references_table = Some(next.clone());
                changed = true;
            }
        }
        if changed {
            cassie.midge.save_constraints(&stored_name, &constraints)?;
            cassie
                .catalog
                .register_constraints(&stored_name, constraints);
        }
    }
    Ok(())
}

fn rewrite_default_sequence(
    constraint: &mut crate::catalog::FieldConstraint,
    sequence_renames: &RelationRenames,
) -> bool {
    let renamed = |name: &str| sequence_renames.get(&name.to_ascii_lowercase()).cloned();
    let mut changed = false;
    if let Some(next) = constraint.default_sequence.as_deref().and_then(renamed) {
        constraint.default_sequence = Some(next);
        changed = true;
    }
    if let Some(next) = constraint
        .default_expression
        .as_deref()
        .and_then(crate::catalog::parse_nextval_default_expression)
        .and_then(|sequence| renamed(&sequence))
    {
        constraint.default_expression = Some(crate::catalog::canonical_nextval_expression(&next));
        changed = true;
    }
    changed
}
