//! Deferred index durable-expression readback witnesses.
use super::*;
use crate::catalog::IndexKind;
use crate::types::{DataType, FieldSchema};
use std::collections::BTreeMap;

fn fixture() -> Midge {
    let midge = Midge::new_in_memory("postgres").expect("in-memory Midge");
    midge
        .create_collection(
            "readback_items",
            Schema {
                fields: vec![FieldSchema {
                    name: "n".into(),
                    data_type: DataType::Int,
                    nullable: true,
                }],
            },
        )
        .expect("collection");
    midge
}
fn index(midge: &Midge) -> IndexMeta {
    IndexMeta {
        definition_semantics_version: 0,
        collection: midge.canonical_collection_name("readback_items"),
        name: "readback_idx".into(),
        field: "n".into(),
        fields: vec!["n".into()],
        expressions: Vec::new(),
        include_fields: Vec::new(),
        predicate: None,
        kind: IndexKind::Scalar,
        unique: false,
        options: BTreeMap::new(),
    }
}
fn encoded(sql: &str) -> String {
    let expr = crate::sql::parser::parse_expression(sql).expect("expression");
    serde_json::to_string(&expr).expect("encoded expression")
}
fn seed(midge: &Midge, meta: &IndexMeta, stored_name: &str) {
    let key = Midge::index_key(&meta.collection, stored_name);
    let raw = serde_json::to_vec(meta).expect("metadata bytes");
    midge
        .raw_put(StorageFamily::Data, &key, &raw)
        .expect("seed legacy record");
}
fn assert_forbidden(result: Result<Option<IndexMeta>, CassieError>) {
    assert!(
        matches!(result, Err(CassieError::Unsupported(message)) if message.contains("index expressions cannot contain subqueries"))
    );
}

#[test]
fn should_reject_encoded_sublinks_at_exact_fallback_with_list_readback() {
    // Arrange
    for sql in [
        "(SELECT 1)",
        "(SELECT var_pop(n) OVER() FROM records)",
        "(SELECT var_pop(n) FILTER(WHERE keep) OVER() FROM records)",
    ] {
        for predicate in [false, true] {
            for fallback in [false, true] {
                let midge = fixture();
                let mut meta = index(&midge);
                if predicate {
                    meta.predicate = Some(encoded(sql));
                } else {
                    meta.expressions.push(encoded(sql));
                }
                seed(
                    &midge,
                    &meta,
                    if fallback {
                        "different_key"
                    } else {
                        &meta.name
                    },
                );
                // Act
                let single = midge.get_index("readback_items", &meta.name);
                let listed = midge.list_indexes();
                // Assert
                assert_forbidden(single);
                assert!(
                    matches!(listed, Err(CassieError::Unsupported(message)) if message.contains("index expressions cannot contain subqueries"))
                );
            }
        }
    }
}

#[test]
fn should_reject_malformed_exact_index_metadata() {
    // Arrange
    let midge = fixture();
    let meta = index(&midge);
    let exact_key = Midge::index_key(&meta.collection, &meta.name);
    midge
        .raw_put(StorageFamily::Data, &exact_key, b"\x7b")
        .expect("malformed exact record");
    // Act
    let exact = midge.get_index("readback_items", &meta.name);
    // Assert
    assert!(
        matches!(exact, Err(CassieError::Parse(message)) if message.contains("invalid index metadata"))
    );
}

#[test]
fn should_read_valid_index_while_skipping_malformed_list_entries() {
    // Arrange
    let midge = fixture();
    let meta = index(&midge);
    let other = Midge::index_key(&meta.collection, "bad_legacy");
    midge
        .raw_put(StorageFamily::Data, &other, b"\x7b")
        .expect("malformed legacy record");
    let mut valid = meta.clone();
    valid.name = "valid_idx".into();
    seed(&midge, &valid, "valid_alias");
    // Act
    let found = midge.get_index("readback_items", "valid_idx");
    let listed = midge.list_indexes();
    // Assert
    assert_eq!(found.expect("fallback"), Some(valid.clone()));
    assert_eq!(listed.expect("list"), vec![valid]);
}

#[test]
fn should_preserve_unknown_version_before_rejecting_unsafe_index_expression() {
    // Arrange
    for fallback in [false, true] {
        let midge = fixture();
        let mut meta = index(&midge);
        meta.definition_semantics_version = 2;
        meta.expressions.push(encoded("(SELECT 1)"));
        seed(
            &midge,
            &meta,
            if fallback {
                "different_key"
            } else {
                &meta.name
            },
        );
        // Act
        let single = midge.get_index("readback_items", &meta.name);
        let listed = midge.list_indexes();
        // Assert
        assert!(
            matches!(single, Err(CassieError::Unsupported(message)) if message.contains("unknown persisted definition semantics version 2"))
        );
        assert!(
            matches!(listed, Err(CassieError::Unsupported(message)) if message.contains("unknown persisted definition semantics version 2"))
        );
    }
}

#[test]
fn should_roundtrip_valid_legacy_index_through_normal_publication() {
    // Arrange
    let midge = fixture();
    let meta = index(&midge);
    // Act
    midge.put_index(&meta).expect("publish valid index");
    let read = midge
        .get_index("readback_items", &meta.name)
        .expect("get")
        .expect("index");
    let listed = midge.list_indexes().expect("list");
    // Assert
    assert_eq!(read.name, meta.name);
    assert_eq!(read.definition_semantics_version, 0);
    assert_eq!(listed, vec![read]);
}

#[test]
fn should_ignore_nonmatching_unsafe_records_in_fallback_get() {
    // Arrange
    let midge = fixture();
    let mut unsafe_index = index(&midge);
    unsafe_index.name = "unrelated_idx".into();
    unsafe_index.expressions.push(encoded("(SELECT 1)"));
    seed(&midge, &unsafe_index, "unsafe_alias");
    let mut valid = index(&midge);
    valid.name = "valid_idx".into();
    valid.expressions.push("not an encoded expression".into());
    seed(&midge, &valid, "valid_alias");
    // Act
    let result = midge.get_index("readback_items", "valid_idx");
    // Assert
    assert_eq!(result.expect("matching valid index"), Some(valid));
    assert!(matches!(
        midge.list_indexes(),
        Err(CassieError::Unsupported(_))
    ));
}
