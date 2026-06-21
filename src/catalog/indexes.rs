use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum IndexKind {
    Scalar,
    FullText,
    Vector,
    Hybrid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IndexMeta {
    pub collection: String,
    pub name: String,
    pub field: String,
    #[serde(default)]
    pub fields: Vec<String>,
    #[serde(default)]
    pub include_fields: Vec<String>,
    #[serde(default)]
    pub predicate: Option<String>,
    pub kind: IndexKind,
    pub unique: bool,
    pub options: std::collections::BTreeMap<String, String>,
}

impl IndexMeta {
    pub fn normalized_fields(&self) -> Vec<String> {
        if self.fields.is_empty() {
            vec![self.field.clone()]
        } else {
            self.fields.clone()
        }
    }

    pub fn normalized_include_fields(&self) -> Vec<String> {
        self.include_fields.clone()
    }

    pub fn primary_field(&self) -> String {
        self.normalized_fields()
            .into_iter()
            .next()
            .unwrap_or_else(|| self.field.clone())
    }

    pub fn rename_field(&mut self, current: &str, next: &str) -> bool {
        let mut changed = false;
        let mut fields = self.normalized_fields();

        for field in &mut fields {
            if field.eq_ignore_ascii_case(current) {
                *field = next.to_string();
                changed = true;
            }
        }

        if changed {
            self.field = fields.first().cloned().unwrap_or_else(|| next.to_string());
            self.fields = fields;
        }

        changed
    }
}
