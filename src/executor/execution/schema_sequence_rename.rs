use super::{
    object_in_schema, rewrite_relation_name_for_schema, Cassie, QueryError, RelationRenames,
};

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
