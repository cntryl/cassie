//! Adapter helpers for durable key families.

use super::{key_encoding, Midge};

impl Midge {
    pub(super) fn collection_schema_key(collection: &str) -> Vec<u8> {
        key_encoding::collection_schema_key(collection)
    }

    pub(super) fn row_schema_key(collection: &str) -> Vec<u8> {
        key_encoding::row_schema_key(collection)
    }

    pub(super) fn projection_key(collection: &str) -> Vec<u8> {
        key_encoding::projection_key(collection)
    }

    pub(super) fn projection_prefix() -> Vec<u8> {
        key_encoding::projection_prefix()
    }

    pub(super) fn projection_comparison_report_key(report_id: &str) -> Vec<u8> {
        key_encoding::projection_comparison_report_key(report_id)
    }

    pub(super) fn projection_comparison_report_prefix() -> Vec<u8> {
        key_encoding::projection_comparison_report_prefix()
    }

    pub(super) fn projection_consistency_report_key(report_id: &str) -> Vec<u8> {
        key_encoding::projection_consistency_report_key(report_id)
    }

    pub(super) fn projection_consistency_report_prefix() -> Vec<u8> {
        key_encoding::projection_consistency_report_prefix()
    }

    pub(super) fn projection_event_key(projection: &str, source_identity: &str, event_id: &str) -> Vec<u8> {
        key_encoding::projection_event_key(projection, source_identity, event_id)
    }

    pub(super) fn projection_event_prefix(projection: &str) -> Vec<u8> {
        key_encoding::projection_event_prefix(projection)
    }

    pub(super) fn operational_assignment_key(assignment_id: &str) -> Vec<u8> {
        key_encoding::operational_assignment_key(assignment_id)
    }

    pub(super) fn operational_assignment_prefix() -> Vec<u8> {
        key_encoding::operational_assignment_prefix()
    }

    pub(super) fn row_hash_key(collection: &str, row_id: &str) -> Vec<u8> {
        key_encoding::row_hash_key(collection, row_id)
    }

    pub(super) fn row_hash_prefix(collection: &str) -> Vec<u8> {
        key_encoding::row_hash_prefix(collection)
    }

    pub(super) fn range_hash_key(collection: &str, range_id: u64) -> Vec<u8> {
        key_encoding::range_hash_key(collection, range_id)
    }

    pub(super) fn range_hash_prefix(collection: &str) -> Vec<u8> {
        key_encoding::range_hash_prefix(collection)
    }

    pub(super) fn root_hash_key(collection: &str) -> Vec<u8> {
        key_encoding::root_hash_key(collection)
    }

    pub(super) fn schema_collection_prefix() -> Vec<u8> {
        key_encoding::schema_collection_prefix()
    }

    pub(super) fn vector_index_key(collection: &str, field: &str) -> Vec<u8> {
        key_encoding::vector_index_key(collection, field)
    }

    pub(super) fn vector_index_prefix() -> Vec<u8> {
        key_encoding::vector_index_prefix()
    }

    pub(super) fn vector_index_state_key(relation_id: u64, field_id: u32) -> Vec<u8> {
        key_encoding::vector_index_state_key(relation_id, field_id)
    }

    pub(super) fn vector_index_state_prefix(relation_id: u64) -> Vec<u8> {
        key_encoding::vector_index_state_prefix(relation_id)
    }

    pub(super) fn data_epoch_key() -> Vec<u8> {
        key_encoding::data_epoch_key()
    }

    pub(super) fn maintenance_debt_key(collection: &str, artifact: &str) -> Vec<u8> {
        key_encoding::maintenance_debt_key(collection, artifact)
    }

    pub(super) fn maintenance_debt_prefix() -> Vec<u8> {
        key_encoding::maintenance_debt_prefix()
    }

    pub(super) fn unique_constraint_reservation_field_prefix(collection: &str, field: &str) -> Vec<u8> {
        key_encoding::unique_constraint_reservation_field_prefix(collection, field)
    }

    pub(super) fn unique_scalar_index_reservation_prefix(collection: &str, index_name: &str) -> Vec<u8> {
        key_encoding::unique_scalar_index_reservation_prefix(collection, index_name)
    }

    pub(super) fn index_publication_key(collection: &str, index: &str) -> Vec<u8> {
        key_encoding::index_publication_key(collection, index)
    }

    pub(super) fn index_publication_prefix() -> Vec<u8> {
        key_encoding::index_publication_prefix()
    }

    pub(super) fn vector_index_collection_prefix(collection: &str) -> Vec<u8> {
        key_encoding::vector_index_collection_prefix(collection)
    }

    pub(super) fn normalized_vector_key(relation_id: u64, field_id: u32, id: &str) -> Vec<u8> {
        key_encoding::normalized_vector_key(relation_id, field_id, id)
    }

    pub(super) fn normalized_vector_prefix(relation_id: u64, field_id: u32) -> Vec<u8> {
        key_encoding::normalized_vector_prefix(relation_id, field_id)
    }

    pub(super) fn normalized_vector_collection_prefix(relation_id: u64) -> Vec<u8> {
        key_encoding::normalized_vector_collection_prefix(relation_id)
    }

    pub(super) fn index_key(collection: &str, name: &str) -> Vec<u8> {
        key_encoding::index_key(collection, name)
    }

    pub(super) fn rollup_prefix() -> Vec<u8> {
        key_encoding::rollup_prefix()
    }

    pub(super) fn rollup_key(name: &str) -> Vec<u8> {
        key_encoding::rollup_key(name)
    }

    pub(super) fn retention_prefix() -> Vec<u8> {
        key_encoding::retention_prefix()
    }

    pub(super) fn retention_key(name: &str) -> Vec<u8> {
        key_encoding::retention_key(name)
    }

    pub(super) fn index_prefix() -> Vec<u8> {
        key_encoding::index_prefix()
    }

    pub(super) fn index_collection_prefix(collection: &str) -> Vec<u8> {
        key_encoding::index_collection_prefix(collection)
    }

    pub(super) fn scalar_index_collection_prefix(relation_id: u64) -> Vec<u8> {
        key_encoding::scalar_index_collection_prefix(relation_id)
    }

    pub(super) fn scalar_index_data_prefix(relation_id: u64, index_id: u64) -> Vec<u8> {
        key_encoding::scalar_index_data_prefix(relation_id, index_id)
    }

    pub(super) fn time_series_index_collection_prefix(relation_id: u64) -> Vec<u8> {
        key_encoding::time_series_index_collection_prefix(relation_id)
    }

    pub(super) fn time_series_index_data_prefix(relation_id: u64, index_id: u64) -> Vec<u8> {
        key_encoding::time_series_index_data_prefix(relation_id, index_id)
    }

    pub(super) fn column_batch_metadata_key(relation_id: u64, index_id: u64) -> Vec<u8> {
        key_encoding::column_batch_metadata_key(relation_id, index_id)
    }

    pub(super) fn column_batch_row_ids_key(
        relation_id: u64,
        index_id: u64,
        segment_id: u64,
        revision: u64,
    ) -> Vec<u8> {
        key_encoding::column_batch_row_ids_key(relation_id, index_id, segment_id, revision)
    }

    pub(super) fn column_batch_field_key(
        relation_id: u64,
        index_id: u64,
        segment_id: u64,
        revision: u64,
        field: &str,
    ) -> Vec<u8> {
        key_encoding::column_batch_field_key(relation_id, index_id, segment_id, revision, field)
    }

    pub(super) fn column_batch_index_prefix(relation_id: u64, index_id: u64) -> Vec<u8> {
        key_encoding::column_batch_index_prefix(relation_id, index_id)
    }

    pub(super) fn column_batch_collection_prefix(relation_id: u64) -> Vec<u8> {
        key_encoding::column_batch_collection_prefix(relation_id)
    }

    pub(super) fn function_key(name: &str) -> Vec<u8> {
        key_encoding::function_key(name)
    }

    pub(super) fn function_prefix() -> Vec<u8> {
        key_encoding::function_prefix()
    }

    pub(super) fn procedure_key(name: &str) -> Vec<u8> {
        key_encoding::procedure_key(name)
    }

    pub(super) fn procedure_prefix() -> Vec<u8> {
        key_encoding::procedure_prefix()
    }

    pub(super) fn view_key(name: &str) -> Vec<u8> {
        key_encoding::view_key(name)
    }

    pub(super) fn view_prefix() -> Vec<u8> {
        key_encoding::view_prefix()
    }

    pub(super) fn role_key(name: &str) -> Vec<u8> {
        key_encoding::role_key(name)
    }

    pub(super) fn role_prefix() -> Vec<u8> {
        key_encoding::role_prefix()
    }

    pub(super) fn role_object_grant_prefix() -> Vec<u8> {
        key_encoding::role_object_grant_prefix()
    }

    pub(super) fn constraints_key(collection: &str) -> Vec<u8> {
        key_encoding::constraints_key(collection)
    }

    pub(super) fn database_key(name: &str) -> Vec<u8> {
        key_encoding::database_key(name)
    }

    pub(super) fn database_prefix() -> Vec<u8> {
        key_encoding::database_prefix()
    }

    pub(super) fn databases_key() -> Vec<u8> {
        key_encoding::databases_key()
    }

    pub(super) fn namespace_key(namespace: &str) -> Vec<u8> {
        key_encoding::namespace_key(namespace)
    }

    pub(super) fn namespace_prefix() -> Vec<u8> {
        key_encoding::namespace_prefix()
    }

    pub(super) fn namespaces_key() -> Vec<u8> {
        key_encoding::namespaces_key()
    }

    pub(super) fn schema_epoch_key() -> Vec<u8> {
        key_encoding::schema_epoch_key()
    }

    pub(super) fn schema_cleanup_key(collection: &str, cleanup_id: &str) -> Vec<u8> {
        key_encoding::schema_cleanup_key(collection, cleanup_id)
    }

    pub(super) fn schema_cleanup_prefix() -> Vec<u8> {
        key_encoding::schema_cleanup_prefix()
    }

    pub(super) fn schema_operation_key(current: &str, next: &str) -> Vec<u8> {
        key_encoding::schema_operation_key(current, next)
    }

    pub(super) fn schema_operation_prefix() -> Vec<u8> {
        key_encoding::schema_operation_prefix()
    }

    pub(super) fn field_rename_operation_key(collection: &str, current: &str, next: &str) -> Vec<u8> {
        key_encoding::field_rename_operation_key(collection, current, next)
    }

    pub(super) fn field_add_operation_key(collection: &str, field: &str) -> Vec<u8> {
        key_encoding::field_add_operation_key(collection, field)
    }

    pub(super) fn field_add_operation_prefix() -> Vec<u8> {
        key_encoding::field_add_operation_prefix()
    }

    pub(super) fn field_rename_operation_prefix() -> Vec<u8> {
        key_encoding::field_rename_operation_prefix()
    }

    pub(super) fn field_drop_operation_key(collection: &str, field: &str) -> Vec<u8> {
        key_encoding::field_drop_operation_key(collection, field)
    }

    pub(super) fn field_drop_operation_prefix() -> Vec<u8> {
        key_encoding::field_drop_operation_prefix()
    }

    pub(super) fn collections_key() -> Vec<u8> {
        key_encoding::collections_key()
    }

    pub(super) fn row_prefix(relation_id: u64) -> Vec<u8> {
        key_encoding::row_prefix(relation_id)
    }

    pub(super) fn row_key(relation_id: u64, id: &str) -> Vec<u8> {
        key_encoding::row_key(relation_id, id)
    }

    pub(super) fn doc_prefix(collection: &str) -> Vec<u8> {
        key_encoding::doc_prefix(collection)
    }

    pub(super) fn doc_key(collection: &str, id: &str) -> Vec<u8> {
        key_encoding::doc_key(collection, id)
    }
}
