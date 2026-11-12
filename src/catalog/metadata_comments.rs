use super::Catalog;
use crate::catalog::{ObjectCommentAddress, ObjectCommentMeta};

impl Catalog {
    pub fn register_object_comment(&self, metadata: ObjectCommentMeta) {
        self.object_comments
            .write()
            .insert(metadata.address.key(), metadata);
        self.bump_version();
    }

    pub fn unregister_object_comment(&self, address: &ObjectCommentAddress) {
        self.object_comments.write().remove(&address.key());
        self.bump_version();
    }

    #[must_use]
    pub fn object_comment(
        &self,
        database: &str,
        class_oid: u32,
        object_oid: u32,
        object_sub_id: i32,
    ) -> Option<String> {
        let address = ObjectCommentAddress {
            database: database.to_ascii_lowercase(),
            class_oid,
            object_oid,
            object_sub_id,
        };
        self.object_comments
            .read()
            .get(&address.key())
            .map(|metadata| metadata.description.clone())
    }

    #[must_use]
    pub fn object_comment_by_oid(&self, database: &str, object_oid: u32) -> Option<String> {
        let database = database.to_ascii_lowercase();
        let comments = self.object_comments.read();
        comments
            .values()
            .filter(|metadata| {
                metadata.address.database.eq_ignore_ascii_case(&database)
                    && metadata.address.object_oid == object_oid
                    && metadata.address.object_sub_id == 0
            })
            .min_by_key(|metadata| metadata.address.class_oid)
            .map(|metadata| metadata.description.clone())
    }

    #[must_use]
    pub fn object_comments_for_database(&self, database: &str) -> Vec<ObjectCommentMeta> {
        let database = database.to_ascii_lowercase();
        self.filtered_object_comments(|metadata| {
            metadata.address.database.eq_ignore_ascii_case(&database)
        })
    }

    #[must_use]
    pub fn object_comments(&self) -> Vec<ObjectCommentMeta> {
        self.filtered_object_comments(|_| true)
    }

    fn filtered_object_comments(
        &self,
        mut include: impl FnMut(&ObjectCommentMeta) -> bool,
    ) -> Vec<ObjectCommentMeta> {
        let mut comments = self
            .object_comments
            .read()
            .values()
            .filter(|metadata| include(metadata))
            .cloned()
            .collect::<Vec<_>>();
        comments.sort_by_key(|metadata| {
            (
                metadata.address.class_oid,
                metadata.address.object_oid,
                metadata.address.object_sub_id,
            )
        });
        comments
    }

    pub(crate) fn unregister_relation_comments(&self, database: &str, object_oid: u32) {
        let database = database.to_ascii_lowercase();
        self.object_comments.write().retain(|_, metadata| {
            !(metadata.address.database.eq_ignore_ascii_case(&database)
                && metadata.address.class_oid == 1259
                && metadata.address.object_oid == object_oid)
        });
        self.bump_version();
    }

    pub(crate) fn unregister_object_comments_for_database(&self, database: &str) {
        let database = database.to_ascii_lowercase();
        self.object_comments
            .write()
            .retain(|_, metadata| !metadata.address.database.eq_ignore_ascii_case(&database));
        self.bump_version();
    }
}
