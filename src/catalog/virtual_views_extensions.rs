//! Catalog descriptors for extension compatibility; no extensions are installed yet.
use crate::types::{DataType, ObjectIdKind};

pub(super) fn installed_schema() -> Vec<(String, DataType)> {
    vec![
        oid("oid"),
        ("extname".to_owned(), DataType::Name),
        oid("extowner"),
        oid("extnamespace"),
        ("extrelocatable".to_owned(), DataType::Boolean),
        ("extversion".to_owned(), DataType::Text),
        (
            "extconfig".to_owned(),
            DataType::Array(Box::new(DataType::ObjectId(ObjectIdKind::Oid))),
        ),
        (
            "extcondition".to_owned(),
            DataType::Array(Box::new(DataType::Text)),
        ),
    ]
}

pub(super) fn available_schema() -> Vec<(String, DataType)> {
    vec![
        ("name".to_owned(), DataType::Name),
        ("default_version".to_owned(), DataType::Text),
        ("installed_version".to_owned(), DataType::Text),
        ("comment".to_owned(), DataType::Text),
    ]
}

fn oid(name: &str) -> (String, DataType) {
    (name.to_owned(), DataType::ObjectId(ObjectIdKind::Oid))
}
