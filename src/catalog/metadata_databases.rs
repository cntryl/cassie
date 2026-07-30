use super::Catalog;
use crate::catalog::DatabaseMeta;

impl Catalog {
    pub fn register_database(&self, name: &str, description: Option<String>) {
        self.register_database_metadata(DatabaseMeta::new(name, description));
    }

    pub fn register_database_metadata(&self, database: DatabaseMeta) {
        self.databases
            .write()
            .insert(database.name.clone(), database);
        self.bump_version();
    }

    pub fn unregister_database(&self, name: &str) {
        self.databases.write().remove(name);
        self.bump_version();
    }

    #[must_use]
    pub fn get_database(&self, name: &str) -> Option<DatabaseMeta> {
        self.databases.read().get(name).cloned()
    }

    #[must_use]
    pub fn database_exists(&self, name: &str) -> bool {
        self.databases.read().contains_key(name)
    }

    #[must_use]
    pub fn list_databases(&self) -> Vec<DatabaseMeta> {
        let mut out = self.databases.read().values().cloned().collect::<Vec<_>>();
        out.sort_by_key(|database| database.name.to_ascii_lowercase());
        out
    }
}
