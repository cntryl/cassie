//! PostgreSQL-style generated index names within the relation namespace.
use crate::app::CassieError;
use crate::catalog::{derive_scoped_name, local_name, Catalog};

const MAX_IDENTIFIER_BYTES: usize = 63;

fn prefix(value: &str, bytes: usize) -> &str {
    let mut end = value.len().min(bytes);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

pub(super) fn generate(
    table: &str,
    fields: &[String],
    catalog: &Catalog,
    database: &str,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<String, CassieError> {
    let table_local = local_name(table);
    let columns = if fields.is_empty() {
        "expression".to_owned()
    } else {
        fields.join("_")
    };
    let mut collision = 0_usize;
    loop {
        let label = if collision == 0 {
            "idx".to_owned()
        } else {
            format!("idx{collision}")
        };
        let available = MAX_IDENTIFIER_BYTES
            .checked_sub(label.len() + 2)
            .ok_or_else(|| CassieError::Planner("no available generated index name".to_owned()))?;
        let mut left = table_local.len();
        let mut right = columns.len();
        while left + right > available {
            if left > right {
                left -= 1;
            } else {
                right -= 1;
            }
        }
        let local = format!(
            "{}_{}_{label}",
            prefix(&table_local, left),
            prefix(&columns, right)
        );
        let scoped = derive_scoped_name(table, |_| local);
        if !catalog.index_relation_name_occupied(&scoped, database, true, controls)? {
            return Ok(scoped);
        }
        collision = collision
            .checked_add(1)
            .ok_or_else(|| CassieError::Planner("no available generated index name".to_owned()))?;
    }
}
