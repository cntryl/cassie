//! Projection catalog descriptors and rows; database visibility remains in the dispatcher.

use super::{
    bool, bool_value, int, int_value, string, text, virtual_views_consistency,
    virtual_views_repair, VirtualRow,
};
use crate::catalog::Catalog;
use crate::types::DataType;

pub(super) fn schema(name: &str) -> Option<Vec<(String, DataType)>> {
    match name {
        "pg_catalog.pg_projection_checkpoints" => Some(vec![
            text("projection_id"),
            text("collection"),
            text("kind"),
            text("source_identity"),
            text("source_checkpoint"),
            int("source_position"),
            text("last_applied_event_id"),
            text("replay_batch_id"),
            int("lag"),
            text("freshness"),
            text("last_error"),
        ]),
        "pg_catalog.pg_materialized_projections" => Some(vec![
            text("projection_name"),
            text("state"),
            text("active_version"),
            text("output_collection"),
            text("source_collections"),
            int("schema_epoch"),
            text("last_error"),
        ]),
        "pg_catalog.pg_projection_versions" => Some(vec![
            text("projection_name"),
            text("version_id"),
            text("output_collection"),
            text("state"),
            int("created_ms"),
            int("activated_ms"),
            int("retired_ms"),
            text("last_error"),
        ]),
        "pg_catalog.pg_projection_operations" => Some(vec![
            text("projection_name"),
            text("kind"),
            text("active_version"),
            int("lag"),
            text("freshness"),
            text("rebuild_state"),
            text("verification_state"),
            text("root_state"),
            text("last_error"),
        ]),
        "pg_catalog.pg_projection_hashes" => Some(vec![
            text("projection_name"),
            text("algorithm"),
            int("digest_length"),
            int("canonical_encoder_version"),
            int("hash_version"),
            text("row_state"),
            int("row_count"),
            text("range_state"),
            int("range_count"),
            text("root_state"),
            text("root_digest"),
        ]),
        "pg_catalog.pg_projection_integrity_reports" => Some(vec![
            text("projection_name"),
            text("state"),
            text("target"),
            text("version_id"),
            text("mode"),
            int("mismatch_count"),
            int("missing_count"),
            int("stale_count"),
            bool("repairable"),
            int("elapsed_ms"),
            text("checked_components"),
            text("skipped_components"),
            text("last_error"),
        ]),
        "pg_catalog.pg_projection_comparison_reports" => Some(vec![
            text("report_id"),
            text("target"),
            text("target_version_id"),
            text("state"),
            text("compatibility_status"),
            text("root_digest"),
            text("manifest_digest"),
            int("mismatch_count"),
            int("unverifiable_count"),
            text("diagnostic_sample"),
            int("created_ms"),
            text("last_error"),
        ]),
        "pg_catalog.pg_projection_consistency_reports" => Some(virtual_views_consistency::schema()),
        "pg_catalog.pg_projection_repair_reports" => Some(virtual_views_repair::schema()),
        _ => None,
    }
}

pub(super) fn rows(catalog: &Catalog, name: &str) -> Option<Vec<VirtualRow>> {
    match name {
        "pg_catalog.pg_projection_checkpoints" => Some(pg_projection_checkpoints_rows(catalog)),
        "pg_catalog.pg_materialized_projections" => Some(pg_materialized_projections_rows(catalog)),
        "pg_catalog.pg_projection_versions" => Some(pg_projection_versions_rows(catalog)),
        "pg_catalog.pg_projection_operations" => Some(pg_projection_operations_rows(catalog)),
        "pg_catalog.pg_projection_hashes" => Some(pg_projection_hashes_rows(catalog)),
        "pg_catalog.pg_projection_integrity_reports" => {
            Some(pg_projection_integrity_reports_rows(catalog))
        }
        "pg_catalog.pg_projection_comparison_reports" => {
            Some(pg_projection_comparison_reports_rows(catalog))
        }
        "pg_catalog.pg_projection_consistency_reports" => {
            Some(virtual_views_consistency::rows(catalog))
        }
        "pg_catalog.pg_projection_repair_reports" => Some(virtual_views_repair::rows(catalog)),
        _ => None,
    }
}

fn pg_projection_checkpoints_rows(catalog: &Catalog) -> Vec<VirtualRow> {
    catalog
        .list_projection_metadata()
        .into_iter()
        .map(|projection| {
            vec![
                string("projection_id", projection.projection_id().to_string()),
                string("collection", projection.collection),
                string("kind", projection.kind.as_str()),
                string(
                    "source_identity",
                    projection.source_identity.unwrap_or_default(),
                ),
                string(
                    "source_checkpoint",
                    projection.source_checkpoint.unwrap_or_default(),
                ),
                int_value(
                    "source_position",
                    projection.source_position.unwrap_or_default(),
                ),
                string(
                    "last_applied_event_id",
                    projection.last_applied_event_id.unwrap_or_default(),
                ),
                string(
                    "replay_batch_id",
                    projection.replay_batch_id.unwrap_or_default(),
                ),
                int_value("lag", projection.lag),
                string("freshness", projection.freshness.as_str()),
                string("last_error", projection.last_error.unwrap_or_default()),
            ]
        })
        .collect()
}

fn pg_materialized_projections_rows(catalog: &Catalog) -> Vec<VirtualRow> {
    catalog
        .list_projection_metadata()
        .into_iter()
        .filter_map(|projection| {
            let materialized = projection.materialized.clone()?;
            let active_version = projection.active_version.clone().unwrap_or_default();
            let output_collection = projection
                .active_output_collection()
                .unwrap_or(&materialized.output_collection)
                .to_string();
            let last_error = projection.last_error.clone().unwrap_or_default();
            Some(vec![
                string("projection_name", projection.collection),
                string("state", materialized.state.as_str()),
                string("active_version", active_version),
                string("output_collection", output_collection),
                string(
                    "source_collections",
                    materialized.source_collections.join(","),
                ),
                int_value("schema_epoch", materialized.schema_epoch),
                string("last_error", last_error),
            ])
        })
        .collect()
}

fn pg_projection_versions_rows(catalog: &Catalog) -> Vec<VirtualRow> {
    catalog
        .list_projection_metadata()
        .into_iter()
        .flat_map(|projection| {
            let projection_name = projection.collection.clone();
            projection.versions.into_iter().map(move |version| {
                vec![
                    string("projection_name", projection_name.clone()),
                    string("version_id", version.version_id),
                    string("output_collection", version.output_collection),
                    string("state", version.state.as_str()),
                    int_value("created_ms", version.created_ms),
                    int_value("activated_ms", version.activated_ms.unwrap_or_default()),
                    int_value("retired_ms", version.retired_ms.unwrap_or_default()),
                    string("last_error", version.last_error.unwrap_or_default()),
                ]
            })
        })
        .collect()
}

fn pg_projection_operations_rows(catalog: &Catalog) -> Vec<VirtualRow> {
    catalog
        .list_projection_metadata()
        .into_iter()
        .map(|projection| {
            vec![
                string("projection_name", projection.collection),
                string("kind", projection.kind.as_str()),
                string(
                    "active_version",
                    projection.active_version.unwrap_or_default(),
                ),
                int_value("lag", projection.lag),
                string("freshness", projection.freshness.as_str()),
                string("rebuild_state", projection.rebuild_state.as_str()),
                string("verification_state", projection.verification.state.as_str()),
                string("root_state", projection.hashes.root.state.as_str()),
                string("last_error", projection.last_error.unwrap_or_default()),
            ]
        })
        .collect()
}

fn pg_projection_hashes_rows(catalog: &Catalog) -> Vec<VirtualRow> {
    catalog
        .list_projection_metadata()
        .into_iter()
        .map(|projection| {
            vec![
                string("projection_name", projection.collection),
                string("algorithm", projection.hashes.algorithm.algorithm),
                int_value(
                    "digest_length",
                    i64::from(projection.hashes.algorithm.digest_length),
                ),
                int_value(
                    "canonical_encoder_version",
                    i64::from(projection.hashes.algorithm.canonical_encoder_version),
                ),
                int_value(
                    "hash_version",
                    i64::from(projection.hashes.algorithm.hash_version),
                ),
                string("row_state", projection.hashes.rows.state.as_str()),
                int_value("row_count", projection.hashes.rows.row_count),
                string("range_state", projection.hashes.ranges.state.as_str()),
                int_value("range_count", projection.hashes.ranges.range_count),
                string("root_state", projection.hashes.root.state.as_str()),
                string(
                    "root_digest",
                    projection.hashes.root.digest.unwrap_or_default(),
                ),
            ]
        })
        .collect()
}

fn pg_projection_integrity_reports_rows(catalog: &Catalog) -> Vec<VirtualRow> {
    catalog
        .list_projection_metadata()
        .into_iter()
        .map(|projection| {
            vec![
                string("projection_name", projection.collection),
                string("state", projection.integrity.state.as_str()),
                string("target", projection.integrity.target.unwrap_or_default()),
                string(
                    "version_id",
                    projection.integrity.version_id.unwrap_or_default(),
                ),
                string("mode", projection.integrity.mode),
                int_value("mismatch_count", projection.integrity.mismatch_count),
                int_value("missing_count", projection.integrity.missing_count),
                int_value("stale_count", projection.integrity.stale_count),
                bool_value("repairable", projection.integrity.repairable),
                int_value("elapsed_ms", projection.integrity.elapsed_ms),
                string(
                    "checked_components",
                    projection.integrity.checked_components.join(","),
                ),
                string(
                    "skipped_components",
                    projection.integrity.skipped_components.join(","),
                ),
                string(
                    "last_error",
                    projection.integrity.last_error.unwrap_or_default(),
                ),
            ]
        })
        .collect()
}

fn pg_projection_comparison_reports_rows(catalog: &Catalog) -> Vec<VirtualRow> {
    catalog
        .list_projection_comparison_reports()
        .into_iter()
        .map(|report| {
            vec![
                string("report_id", report.report_id),
                string("target", report.target),
                string(
                    "target_version_id",
                    report.target_version_id.unwrap_or_default(),
                ),
                string("state", report.state),
                string("compatibility_status", report.compatibility_status),
                string("root_digest", report.root_digest.unwrap_or_default()),
                string(
                    "manifest_digest",
                    report.manifest_digest.unwrap_or_default(),
                ),
                int_value("mismatch_count", report.mismatch_count),
                int_value("unverifiable_count", report.unverifiable_count),
                string("diagnostic_sample", report.diagnostic_sample.join(",")),
                int_value("created_ms", report.created_ms),
                string("last_error", report.last_error.unwrap_or_default()),
            ]
        })
        .collect()
}
