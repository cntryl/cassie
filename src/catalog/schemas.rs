use crate::types::{DataType, DomainRef};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldMeta {
    pub name: String,
    /// The type SQL resolution sees; a domain column holds its base type here.
    pub data_type: DataType,
    /// The domain the column was declared with, if any (#828).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<DomainRef>,
    pub is_indexed: bool,
    pub boost: Option<f32>,
}

impl FieldMeta {
    /// A field declared as `data_type`, splitting a domain into its base type
    /// and its identity.
    #[must_use]
    pub fn declared(
        name: String,
        data_type: DataType,
        is_indexed: bool,
        boost: Option<f32>,
    ) -> Self {
        match data_type {
            DataType::Domain(domain) => Self {
                name,
                data_type: domain.base.flattened().clone(),
                domain: Some(domain),
                is_indexed,
                boost,
            },
            DataType::Composite(row)
                if row.fields.iter().any(|f| f.data_type.domain().is_some()) =>
            {
                Self {
                    name,
                    data_type: DataType::Composite(super::relation_row_type::unconstrained(&row)),
                    domain: None,
                    is_indexed,
                    boost,
                }
            }
            other => Self {
                name,
                data_type: other,
                domain: None,
                is_indexed,
                boost,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionSchema {
    pub collection: String,
    pub fields: Vec<FieldMeta>,
}

impl CollectionSchema {
    pub fn new(collection: impl Into<String>) -> Self {
        Self {
            collection: collection.into(),
            fields: Vec::new(),
        }
    }

    pub(crate) fn from_type_schema(
        collection: impl Into<String>,
        schema: &crate::types::Schema,
    ) -> Self {
        Self {
            collection: collection.into(),
            fields: schema
                .fields
                .iter()
                .map(|field| {
                    FieldMeta::declared(field.name.clone(), field.data_type.clone(), false, None)
                })
                .collect(),
        }
    }

    /// The declared schema of a built-in `information_schema`/`pg_catalog`
    /// view, so Describe and execution type its columns identically.
    #[must_use]
    pub fn virtual_view(collection: &str) -> Option<Self> {
        let fields = crate::catalog::virtual_views::schema(collection)?;
        Some(Self {
            collection: collection.to_string(),
            fields: fields
                .into_iter()
                .map(|(name, data_type)| FieldMeta {
                    name,
                    data_type,
                    domain: None,
                    is_indexed: false,
                    boost: None,
                })
                .collect(),
        })
    }

    #[must_use]
    pub fn field(&self, name: &str) -> Option<&FieldMeta> {
        self.fields.iter().find(|f| f.name == name)
    }

    /// Whether this schema declares its own `id` column, which shadows the
    /// legacy `id` alias for the internal row identity (see
    /// [`crate::types::row_identity`]).
    #[must_use]
    pub fn declares_id(&self) -> bool {
        crate::types::row_identity::declares_id(self.fields.iter().map(|field| field.name.as_str()))
    }
}
