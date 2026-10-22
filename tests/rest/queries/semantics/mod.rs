use std::sync::{mpsc, Arc, Barrier};
use std::time::Duration;

use cassie::app::{CassieError, CatalogObjectKind};
use cassie::catalog::{canonical_relation_name, FieldConstraint};
use cassie::midge::adapter::{
    schema_write_conflict_test_guard, schema_write_conflict_worker_guard,
    set_schema_write_commit_barriers, SchemaWritePausePoint,
};
use cassie::rest::{collections, documents};
use uuid::Uuid;

use super::support_sql as support;
use support::*;

include!("semantics.rs");

include!("errors.rs");

fn assert_relation_already_exists(result: &Result<serde_json::Value, CassieError>) {
    assert!(matches!(
        result,
        Err(CassieError::CatalogObjectAlreadyExists {
            kind: CatalogObjectKind::Relation,
            ..
        })
    ));
}

fn assert_table_integrity_constraints(constraints: &[FieldConstraint]) {
    assert!(constraints.iter().any(|constraint| constraint.primary_key));
    assert!(constraints
        .iter()
        .any(|constraint| constraint.references_table.is_some()));
    assert!(constraints
        .iter()
        .any(|constraint| constraint.check.is_some()));
    assert!(constraints.iter().any(|constraint| constraint.not_null));
    assert!(constraints
        .iter()
        .any(|constraint| constraint.default_value.is_some()));
}

include!("restart.rs");
