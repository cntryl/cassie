use super::{
    CassieError, ColumnFamilyHandle, FamilyScope, Midge, StorageFamily, TransactionMode,
    DEFAULT_FAMILY_NAME,
};

impl Midge {
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn schema_tx(
        &self,
        mode: TransactionMode,
    ) -> Result<cntryl_midge::Transaction, CassieError> {
        self.begin_families_tx(&[StorageFamily::Schema], mode)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn data_tx(&self, mode: TransactionMode) -> Result<cntryl_midge::Transaction, CassieError> {
        self.begin_families_tx(&[StorageFamily::Data], mode)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn temp_tx(&self, mode: TransactionMode) -> Result<cntryl_midge::Transaction, CassieError> {
        self.begin_families_tx(&[StorageFamily::Temp], mode)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn default_tx(
        &self,
        mode: TransactionMode,
    ) -> Result<cntryl_midge::Transaction, CassieError> {
        self.transaction_by_name(DEFAULT_FAMILY_NAME, mode)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn begin_families_tx(
        &self,
        families: &[StorageFamily],
        mode: TransactionMode,
    ) -> Result<cntryl_midge::Transaction, CassieError> {
        let scope = FamilyScope::for_families(families)?;
        let family = scope.family().ok_or_else(|| {
            CassieError::Unsupported(
                "transactions currently support exactly one storage family".to_string(),
            )
        })?;

        self.transaction(family, mode)
    }

    pub(super) fn transaction(
        &self,
        family: StorageFamily,
        mode: TransactionMode,
    ) -> Result<cntryl_midge::Transaction, CassieError> {
        let layout = self.ensure_families_ready()?;
        let cf = match family {
            StorageFamily::Schema => &layout.schema,
            StorageFamily::Data => &layout.data,
            StorageFamily::Temp => &layout.temp,
        };

        self.engine
            .begin_tx(cf.id(), mode)
            .map_err(CassieError::from)
    }

    pub(super) fn transaction_by_name(
        &self,
        family: &str,
        mode: TransactionMode,
    ) -> Result<cntryl_midge::Transaction, CassieError> {
        let Some(cf) = self.engine.get_column_family(family) else {
            return Err(CassieError::StorageMissingFamily(format!(
                "required column family '{family}' is missing"
            )));
        };

        self.engine
            .begin_tx(cf.id(), mode)
            .map_err(CassieError::from)
    }

    pub(super) fn get_or_create_family(
        &self,
        family: StorageFamily,
    ) -> Result<ColumnFamilyHandle, CassieError> {
        let name = family.name();
        if let Some(existing) = self.engine.get_column_family(name) {
            return Ok(existing);
        }

        if let Ok(created) = self.engine.create_column_family(name) {
            return Ok(created);
        }

        self.engine.get_column_family(name).ok_or_else(|| {
            CassieError::StorageBootstrap(format!("cannot resolve required column family '{name}'"))
        })
    }

    pub(super) fn begin_schema_readonly_tx(
        &self,
    ) -> Result<cntryl_midge::Transaction, CassieError> {
        self.schema_tx(TransactionMode::ReadOnly)
    }

    pub(super) fn begin_schema_rw_tx(&self) -> Result<cntryl_midge::Transaction, CassieError> {
        self.schema_tx(TransactionMode::ReadWrite)
    }

    pub(super) fn begin_data_readonly_tx(&self) -> Result<cntryl_midge::Transaction, CassieError> {
        self.data_tx(TransactionMode::ReadOnly)
    }

    pub(super) fn begin_data_rw_tx(&self) -> Result<cntryl_midge::Transaction, CassieError> {
        self.data_tx(TransactionMode::ReadWrite)
    }
}
