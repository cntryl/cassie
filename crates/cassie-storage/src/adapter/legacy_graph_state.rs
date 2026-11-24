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

#[cfg(test)]
mod tests {
    use super::super::{key_encoding, StorageFamily};
    use crate::app::Cassie;
    use crate::config::CassieRuntimeConfig;

    fn open(path: &std::path::Path) -> Cassie {
        let cassie =
            Cassie::new_local_with_data_dir_and_config(path, CassieRuntimeConfig::default())
                .expect("open local storage");
        cassie.startup().expect("start up");
        cassie
    }

    #[test]
    fn should_remove_legacy_graph_entries_on_startup() {
        // Arrange
        let path =
            std::env::temp_dir().join(format!("cassie-legacy-graph-{}", uuid::Uuid::new_v4()));
        let cassie = open(&path);
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE kept (id BIGINT PRIMARY KEY)",
                vec![],
            )
            .expect("create table");
        cassie
            .execute_sql(&session, "INSERT INTO kept VALUES (1)", vec![])
            .expect("insert row");
        for key in key_encoding::legacy_graph_sample_keys() {
            cassie
                .midge
                .raw_put(StorageFamily::Data, &key, b"{}")
                .expect("write a legacy entry");
        }
        cassie.shutdown();
        drop(session);
        drop(cassie);

        // Act
        let recovered = open(&path);
        let leftovers = key_encoding::legacy_graph_prefixes()
            .iter()
            .map(|prefix| {
                recovered
                    .midge
                    .raw_scan_prefix(StorageFamily::Data, prefix)
                    .expect("scan legacy prefix")
                    .len()
            })
            .sum::<usize>();
        let session = recovered.create_session("tester", None);
        let rows = recovered
            .execute_sql(&session, "SELECT id FROM kept", vec![])
            .expect("read the former table")
            .rows()
            .len();
        recovered.shutdown();
        drop(session);
        drop(recovered);
        std::fs::remove_dir_all(path).expect("remove fixture");

        // Assert
        assert_eq!(leftovers, 0);
        assert_eq!(rows, 1);
    }
}
