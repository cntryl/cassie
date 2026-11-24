use super::{encode_row, CassieError, RowSchema, FORMAT_VERSION, MAGIC, TYPE_JSON};
use crate::types::{DataType, FieldSchema, Schema};

#[path = "controlled_directory.rs"]
mod controlled_directory;
#[cfg(test)]
#[path = "controlled_directory_tests.rs"]
mod controlled_directory_tests;

#[doc(hidden)]
pub fn owned_row_cell(
    source: &str,
    identity: crate::runtime::accounted::json_arena::PlainIdentity,
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<crate::runtime::accounted::json_arena::PlainCell, CassieError> {
    use crate::runtime::accounted::json_arena::{PlainCellError, PlainIdentity};
    use controlled_directory::{Directory, HandoffError};
    let data_type = match identity {
        PlainIdentity::Json => DataType::Json,
        PlainIdentity::Jsonb => DataType::Jsonb,
    };
    let schema = RowSchema::from_schema(&Schema {
        fields: vec![FieldSchema {
            name: "doc".into(),
            data_type,
            nullable: true,
        }],
    });
    let mut row = Vec::new();
    row.extend_from_slice(MAGIC);
    row.push(FORMAT_VERSION);
    row.extend_from_slice(&1_u32.to_be_bytes());
    row.extend_from_slice(&1_u32.to_be_bytes());
    row.extend_from_slice(&1_u32.to_be_bytes());
    row.extend_from_slice(&[1, 0]);
    row.extend_from_slice(&1_u32.to_be_bytes());
    row.extend_from_slice(&1_u32.to_be_bytes());
    row.push(TYPE_JSON);
    row.extend_from_slice(&0_u32.to_be_bytes());
    row.extend_from_slice(
        &u32::try_from(source.len())
            .map_err(|_| CassieError::Execution("fixture too large".into()))?
            .to_be_bytes(),
    );
    row.extend_from_slice(source.as_bytes());
    let directory = Directory::parse(&row, controls)?;
    match directory.json_cell(&schema.fields[0], controls) {
        Ok(Some(cell)) => Ok(cell),
        Err(
            HandoffError::Directory(error) | HandoffError::Cell(PlainCellError::Control(error)),
        ) => Err(error),
        _ => Err(CassieError::Execution("invalid row fixture".into())),
    }
}

#[doc(hidden)]
pub fn owned_row_diagnostic(
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<crate::runtime::accounted::json_arena::JsonbDiagnostic, CassieError> {
    use crate::runtime::accounted::json_arena::PlainCellError;
    use controlled_directory::{Directory, HandoffError};
    let schema = RowSchema::from_schema(&Schema {
        fields: vec![FieldSchema {
            name: "doc".into(),
            data_type: DataType::Json,
            nullable: true,
        }],
    });
    let mut row = encode_row(&schema, &serde_json::json!({"doc":"xx"}))?;
    let start = row
        .len()
        .checked_sub(4)
        .ok_or_else(|| CassieError::Execution("invalid row fixture".into()))?;
    row[start..].copy_from_slice(b"[}\xff ");
    let directory = Directory::parse(&row, controls)?;
    match directory.json_cell(&schema.fields[0], controls) {
        Err(HandoffError::Cell(PlainCellError::Syntax(diagnostic))) => Ok(diagnostic),
        Err(
            HandoffError::Cell(PlainCellError::Control(error)) | HandoffError::Directory(error),
        ) => Err(error),
        _ => Err(CassieError::Execution(
            "unexpected row diagnostic fixture result".into(),
        )),
    }
}
