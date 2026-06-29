use std::env;

use cntryl_midge::ColumnFamilyHandle;

use crate::app::CassieError;

pub(crate) const SCHEMA_FAMILY_NAME: &str = "cf0";
pub(crate) const DATA_FAMILY_NAME: &str = "cf1";
pub(crate) const TEMP_FAMILY_NAME: &str = "cf2";
pub(crate) const DEFAULT_FAMILY_NAME: &str = "default";

pub(crate) type RawStorageEntry = (Vec<u8>, Vec<u8>);

pub(crate) fn allow_memory_fallback() -> bool {
    env::var("CASSIE_MIDGE_ALLOW_FALLBACK")
        .is_ok_and(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageFamily {
    Schema,
    Data,
    Temp,
}

impl StorageFamily {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Schema => SCHEMA_FAMILY_NAME,
            Self::Data => DATA_FAMILY_NAME,
            Self::Temp => TEMP_FAMILY_NAME,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct FamilyScope {
    include_schema: bool,
    include_data: bool,
    include_temp: bool,
}

impl FamilyScope {
    pub(super) fn for_families(families: &[StorageFamily]) -> Result<Self, CassieError> {
        if families.is_empty() {
            return Err(CassieError::Unsupported(
                "transaction scope must include at least one storage family".to_string(),
            ));
        }

        let include_schema = families
            .iter()
            .any(|family| matches!(family, StorageFamily::Schema));
        let include_data = families
            .iter()
            .any(|family| matches!(family, StorageFamily::Data));
        let include_temp = families
            .iter()
            .any(|family| matches!(family, StorageFamily::Temp));

        if include_schema && include_data {
            return Err(CassieError::Unsupported(
                "cannot open a transaction across schema and data families".to_string(),
            ));
        }

        if include_temp && (include_schema || include_data) {
            return Err(CassieError::Unsupported(
                "transactions currently support exactly one storage family".to_string(),
            ));
        }

        Ok(Self {
            include_schema,
            include_data,
            include_temp,
        })
    }

    pub(super) fn family(self) -> Option<StorageFamily> {
        if self.include_schema {
            Some(StorageFamily::Schema)
        } else if self.include_data {
            Some(StorageFamily::Data)
        } else if self.include_temp {
            Some(StorageFamily::Temp)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub struct StorageLayout {
    pub schema: ColumnFamilyHandle,
    pub data: ColumnFamilyHandle,
    pub temp: ColumnFamilyHandle,
}
