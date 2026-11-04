//! Declared JSONB families in private, query-local SEARCH/CYCLE lineage.
use super::semantic::SemanticValue;
use crate::app::CassieError;
use crate::runtime::QueryExecutionControls;
use crate::types::{DataType, Value};

#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum KeyType {
    #[default]
    Ordinary,
    Jsonb,
    JsonbArray,
}
impl KeyType {
    pub(super) fn from_type(data_type: &DataType) -> Self {
        match data_type {
            DataType::Jsonb => Self::Jsonb,
            DataType::Array(element) if super::semantic::jsonb_type(element) => Self::JsonbArray,
            _ => Self::Ordinary,
        }
    }
    pub(super) fn key(
        self,
        value: &Value,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<SemanticValue, CassieError> {
        if let Some(controls) = controls {
            controls.check_interruption()?;
        }
        match self {
            Self::Ordinary => Ok(SemanticValue::from_value(value)),
            Self::Jsonb => super::semantic::typed_value(value, Some(&DataType::Jsonb), controls),
            Self::JsonbArray => {
                if matches!(value, Value::Null) {
                    Ok(SemanticValue::Null)
                } else {
                    super::semantic::jsonb_array(value, controls)
                }
            }
        }
    }
}

pub(super) fn key_type(types: &[KeyType], index: usize) -> KeyType {
    types.get(index).copied().unwrap_or_default()
}
pub(super) fn validate_width(types: &[KeyType], width: usize) -> Result<(), CassieError> {
    if !types.is_empty() && types.len() != width {
        return Err(CassieError::Execution(
            "private SEARCH/CYCLE key type width is invalid".into(),
        ));
    }
    Ok(())
}
