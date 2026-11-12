use serde::{Deserialize, Serialize};

/// Stable address for a selected schema, relation, or relation-column comment.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ObjectCommentAddress {
    pub database: String,
    pub class_oid: u32,
    pub object_oid: u32,
    pub object_sub_id: i32,
}

/// Current durable payload for one selected SQL object comment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectCommentMeta {
    pub format_version: u16,
    pub address: ObjectCommentAddress,
    pub description: String,
}

impl ObjectCommentAddress {
    #[must_use]
    pub fn key(&self) -> String {
        format!(
            "{}\0{}\0{}\0{}",
            self.database.to_ascii_lowercase(),
            self.class_oid,
            self.object_oid,
            self.object_sub_id,
        )
    }
}

impl ObjectCommentMeta {
    pub const FORMAT_VERSION: u16 = 1;

    /// # Errors
    ///
    /// Returns an error when the address or payload is outside the selected format.
    pub fn validate(&self, database: &str) -> Result<(), String> {
        if self.format_version != Self::FORMAT_VERSION {
            return Err(format!(
                "object comment format {} is unsupported",
                self.format_version
            ));
        }
        if self.address.database.is_empty()
            || !self.address.database.eq_ignore_ascii_case(database)
            || self.address.object_oid == 0
            || self.description.is_empty()
        {
            return Err("object comment has an invalid address or empty description".to_owned());
        }
        let valid_sub_id = match self.address.class_oid {
            2615 => self.address.object_sub_id == 0,
            1259 => self.address.object_sub_id >= 0,
            _ => false,
        };
        if !valid_sub_id {
            return Err(
                "object comment address is outside the selected catalog classes".to_owned(),
            );
        }
        Ok(())
    }
}
