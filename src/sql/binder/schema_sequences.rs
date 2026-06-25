use super::*;

pub(super) fn bind_create_sequence(
    mut statement: CreateSequenceStatement,
    catalog: &Catalog,
) -> Result<CreateSequenceStatement, CassieError> {
    let name = statement.name.trim().to_string();
    if name.is_empty() {
        return Err(CassieError::Planner(
            "CREATE SEQUENCE requires a name".into(),
        ));
    }
    if !statement.if_not_exists && catalog.sequence_exists(&name) {
        return Err(CassieError::Planner(format!(
            "sequence '{name}' already exists"
        )));
    }
    statement.name = name;
    Ok(statement)
}

pub(super) fn bind_drop_sequence(
    mut statement: DropSequenceStatement,
    catalog: &Catalog,
) -> Result<DropSequenceStatement, CassieError> {
    let name = statement.name.trim().to_string();
    if name.is_empty() {
        return Err(CassieError::Planner("DROP SEQUENCE requires a name".into()));
    }
    if !statement.if_exists && !catalog.sequence_exists(&name) {
        return Err(CassieError::NotFound(format!(
            "sequence '{name}' does not exist"
        )));
    }
    statement.name = name;
    Ok(statement)
}

pub(super) fn validate_alter_column_operation(
    table: &str,
    operation: &AlterTableOperation,
    existing_fields: &HashSet<String>,
    catalog: &Catalog,
) -> Result<(), CassieError> {
    match operation {
        AlterTableOperation::AlterColumnSetDefault {
            field,
            default_sequence,
            ..
        } => {
            validate_alter_column_field(table, "SET DEFAULT", field, existing_fields)?;
            if let Some(sequence) = default_sequence {
                if !catalog.sequence_exists(sequence) {
                    return Err(CassieError::NotFound(format!(
                        "sequence '{sequence}' does not exist"
                    )));
                }
            }
        }
        AlterTableOperation::AlterColumnDropDefault { field }
        | AlterTableOperation::AlterColumnSetNotNull { field }
        | AlterTableOperation::AlterColumnDropNotNull { field } => {
            validate_alter_column_field(table, "ALTER COLUMN", field, existing_fields)?;
        }
        _ => {}
    }

    Ok(())
}

fn validate_alter_column_field(
    table: &str,
    operation: &str,
    field: &str,
    existing_fields: &HashSet<String>,
) -> Result<(), CassieError> {
    let name = field.trim();
    if name.is_empty() {
        return Err(CassieError::Planner(format!(
            "ALTER TABLE ALTER COLUMN {operation} requires a field"
        )));
    }
    if !existing_fields.contains(&name.to_ascii_lowercase()) {
        return Err(CassieError::Planner(format!(
            "ALTER TABLE '{table}' has no field '{name}'"
        )));
    }
    Ok(())
}
