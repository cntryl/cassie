//! Removal of the state the legacy `CREATE GRAPH` feature persisted.
//!
//! The legacy graph feature was removed without conversion: its definitions
//! and adjacency entries are never read again, and the tables that backed it
//! stay ordinary tables. Leaving the entries in place would make database
//! images and copies treat them as unknown catalog families, so startup
//! deletes them.

use cntryl_midge::TransactionMode;

use super::{collect_scan, key_encoding, CassieError, Midge, Query};

impl Midge {
    /// Deletes every legacy graph definition and adjacency entry from every
    /// database and returns how many entries were removed.
    ///
    /// # Errors
    ///
    /// Returns an error when storage fails.
    pub fn purge_legacy_graph_state(&self) -> Result<usize, CassieError> {
        let mut removed = 0;
        for database in self.database_names() {
            let mut keys = Vec::new();
            {
                let tx = self.begin_database_readonly_tx(&database)?;
                for prefix in key_encoding::legacy_graph_prefixes() {
                    let entries = collect_scan(
                        tx.scan(&Query::new().prefix(prefix.into()))
                            .map_err(CassieError::from)?,
                    )?;
                    keys.extend(entries.into_iter().map(|(key, _value)| key));
                }
            }
            if keys.is_empty() {
                continue;
            }
            let family = self.database_family(&database)?;
            let mut tx = self.begin_adapter_tx(family.id(), TransactionMode::ReadWrite)?;
            removed += keys.len();
            for key in keys {
                tx.delete(key).map_err(CassieError::from)?;
            }
            tx.commit(self.write_options_sync())
                .map_err(CassieError::from)?;
        }
        Ok(removed)
    }
}
