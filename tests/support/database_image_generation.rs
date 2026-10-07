//! Logical images reject source-generation changes instead of publishing mixed rows.
use cassie::app::{Cassie, CassieSession, DatabaseBackupStream};

pub struct Fixture {
    pub cassie: Cassie,
    pub session: CassieSession,
    path: String,
}

impl Fixture {
    /// Opens and seeds one local disk-backed source database.
    ///
    /// # Panics
    ///
    /// Panics when fixture initialization fails.
    pub fn seeded() -> Self {
        super::support_sql::use_local_storage();
        let path = super::support_sql::data_dir("database-image-generation");
        let cassie = Cassie::new_with_data_dir(&path).expect("Cassie");
        cassie.startup().expect("startup");
        cassie
            .midge
            .create_database("analytics", None)
            .expect("database");
        cassie
            .midge
            .create_namespace("analytics.public")
            .expect("namespace");
        cassie.hydrate_catalog().expect("catalog");
        let session = cassie.create_session("tester", Some("analytics".to_owned()));
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE docs (v BIGINT, padding TEXT)",
                vec![],
            )
            .expect("table");
        cassie
            .midge
            .put_fresh_documents(
                "analytics.public.docs",
                (0..300)
                    .map(|row| {
                        (
                            Some(format!("{row:08}")),
                            serde_json::json!({"v": 1, "padding": "x".repeat(2048)}),
                        )
                    })
                    .collect(),
            )
            .expect("seed");
        Self {
            cassie,
            session,
            path,
        }
    }

    /// Verifies a prefix remains invisible and can be safely discarded.
    ///
    /// # Panics
    ///
    /// Panics when framing, visibility or cleanup violates the contract.
    pub fn assert_incomplete_image(&self, prefix: &[u8]) {
        let mut restore = self
            .cassie
            .begin_database_restore("rejected")
            .expect("restore staging");
        restore.push_chunk(prefix).expect("valid incomplete prefix");
        assert!(restore.finish().is_err(), "no complete footer is available");
        assert!(self
            .cassie
            .midge
            .get_database("rejected")
            .expect("target lookup")
            .is_none());
        restore.abort().expect("remove staging");
        restore.abort().expect("idempotent abort");
    }

    /// Verifies source changes permanently reject the stream.
    ///
    /// # Panics
    ///
    /// Panics if a changed source can emit another chunk.
    pub fn assert_rejected(backup: &mut DatabaseBackupStream) {
        let next = backup.next_chunk();
        assert!(next.is_err(), "source mutation must reject backup");
        let error = next.unwrap_err();
        assert!(error.to_string().contains("changed"), "{error}");
        assert!(
            backup.next_chunk().is_err(),
            "failed stream cannot resume toward a footer"
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
