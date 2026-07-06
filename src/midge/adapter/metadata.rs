use super::{
    key_encoding, payload_contains_index_membership, payload_contains_vector_membership,
    CassieError, CollectionCardinalityStats, FieldConstraint, IndexKind, IndexMeta, Midge,
    ProjectionMeta, Query, RetentionPolicyMeta, RoleMeta, RollupMeta, Schema, StorageFamily,
    WriteOptions,
};

impl Midge {
    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn put_index(&self, metadata: &IndexMeta) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let key = Self::index_key(&metadata.collection, &metadata.name);
        let value =
            serde_json::to_vec(metadata).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(key, value, None).map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        if metadata.kind == IndexKind::Scalar {
            self.rebuild_scalar_index_for_index(metadata)?;
        }
        if metadata.kind == IndexKind::TimeSeries {
            self.rebuild_time_series_index_for_index(metadata)?;
        }
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn get_index(
        &self,
        collection: &str,
        name: &str,
    ) -> Result<Option<IndexMeta>, CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        let raw = tx
            .get(&Self::index_key(collection, name))
            .map_err(CassieError::from)?;
        let Some(raw) = raw else {
            return Ok(None);
        };

        serde_json::from_slice(&raw)
            .map(Some)
            .map_err(|error| CassieError::Parse(format!("invalid index metadata: {error}")))
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn list_indexes(&self) -> Result<Vec<IndexMeta>, CassieError> {
        let entries = self.raw_scan_prefix(StorageFamily::Schema, &Self::index_prefix())?;
        let mut out = Vec::with_capacity(entries.len());

        for (_key, raw_value) in entries {
            let Ok(record) = serde_json::from_slice(&raw_value) else {
                continue;
            };
            out.push(record);
        }

        Ok(out)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn delete_index(&self, collection: &str, name: &str) -> Result<(), CassieError> {
        let metadata = self.get_index(collection, name)?;
        let mut tx = self.begin_schema_rw_tx()?;
        tx.delete(Self::index_key(collection, name))
            .map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        if let Some(index) = metadata {
            if index.kind == IndexKind::Scalar {
                self.delete_scalar_index_data(collection, name)?;
            }
            if index.kind == IndexKind::TimeSeries {
                self.delete_time_series_index_data(collection, name)?;
            }
        }
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn put_rollup(&self, metadata: &RollupMeta) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let value =
            serde_json::to_vec(metadata).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(Self::rollup_key(&metadata.name), value, None)
            .map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn list_rollups(&self) -> Result<Vec<RollupMeta>, CassieError> {
        let entries = self.raw_scan_prefix(StorageFamily::Schema, &Self::rollup_prefix())?;
        let mut out = Vec::with_capacity(entries.len());
        for (_key, raw_value) in entries {
            let Ok(record) = serde_json::from_slice(&raw_value) else {
                continue;
            };
            out.push(record);
        }
        Ok(out)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn delete_rollup(&self, name: &str) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        tx.delete(Self::rollup_key(name))
            .map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn put_retention_policy(&self, metadata: &RetentionPolicyMeta) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let value =
            serde_json::to_vec(metadata).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(Self::retention_key(&metadata.name), value, None)
            .map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn list_retention_policies(&self) -> Result<Vec<RetentionPolicyMeta>, CassieError> {
        let entries = self.raw_scan_prefix(StorageFamily::Schema, &Self::retention_prefix())?;
        let mut out = Vec::with_capacity(entries.len());
        for (_key, raw_value) in entries {
            let Ok(record) = serde_json::from_slice(&raw_value) else {
                continue;
            };
            out.push(record);
        }
        Ok(out)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn delete_retention_policy(&self, name: &str) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        tx.delete(Self::retention_key(name))
            .map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn put_graph(&self, metadata: &crate::catalog::GraphMeta) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let value =
            serde_json::to_vec(metadata).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(Self::graph_key(&metadata.name), value, None)
            .map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn list_graphs(&self) -> Result<Vec<crate::catalog::GraphMeta>, CassieError> {
        let entries = self.raw_scan_prefix(StorageFamily::Schema, &Self::graph_prefix())?;
        let mut out = Vec::with_capacity(entries.len());
        for (_key, raw_value) in entries {
            let Ok(record) = serde_json::from_slice(&raw_value) else {
                continue;
            };
            out.push(record);
        }
        Ok(out)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn delete_vector_index(&self, collection: &str, field: &str) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        tx.delete(Self::vector_index_key(collection, field))
            .map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;

        let mut data_tx = self.begin_data_rw_tx()?;
        Self::delete_normalized_vector_keys_with_prefix(
            &mut data_tx,
            Self::normalized_vector_prefix(collection, field),
        )?;
        data_tx
            .commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn put_projection_comparison_report(
        &self,
        report: &crate::catalog::ProjectionComparisonReportMeta,
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let value =
            serde_json::to_vec(report).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(
            Self::projection_comparison_report_key(&report.report_id),
            value,
            None,
        )
        .map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn list_projection_comparison_reports(
        &self,
    ) -> Result<Vec<crate::catalog::ProjectionComparisonReportMeta>, CassieError> {
        let entries = self.raw_scan_prefix(
            StorageFamily::Schema,
            &Self::projection_comparison_report_prefix(),
        )?;
        let mut out = Vec::with_capacity(entries.len());
        for (_key, raw_value) in entries {
            let Ok(report) = serde_json::from_slice(&raw_value) else {
                continue;
            };
            out.push(report);
        }
        out.sort_by_key(|report: &crate::catalog::ProjectionComparisonReportMeta| {
            report.report_id.clone()
        });
        Ok(out)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn put_projection_consistency_report(
        &self,
        report: &crate::catalog::ProjectionConsistencyReportMeta,
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let value =
            serde_json::to_vec(report).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(
            Self::projection_consistency_report_key(&report.report_id),
            value,
            None,
        )
        .map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn list_projection_consistency_reports(
        &self,
    ) -> Result<Vec<crate::catalog::ProjectionConsistencyReportMeta>, CassieError> {
        let entries = self.raw_scan_prefix(
            StorageFamily::Schema,
            &Self::projection_consistency_report_prefix(),
        )?;
        let mut out = Vec::with_capacity(entries.len());
        for (_key, raw_value) in entries {
            let Ok(report) = serde_json::from_slice(&raw_value) else {
                continue;
            };
            out.push(report);
        }
        out.sort_by_key(|report: &crate::catalog::ProjectionConsistencyReportMeta| {
            report.report_id.clone()
        });
        Ok(out)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn save_constraints(
        &self,
        collection: &str,
        constraints: &[FieldConstraint],
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let value = serde_json::to_vec(constraints)
            .map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(Self::constraints_key(collection), value, None)
            .map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn load_constraints(&self, collection: &str) -> Result<Vec<FieldConstraint>, CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        let raw = tx
            .get(&Self::constraints_key(collection))
            .map_err(CassieError::from)?;
        let Some(raw) = raw else {
            return Ok(Vec::new());
        };

        serde_json::from_slice(&raw)
            .map_err(|error| CassieError::Parse(format!("invalid constraint metadata: {error}")))
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn get_cardinality_stats(
        &self,
        collection: &str,
    ) -> Result<Option<CollectionCardinalityStats>, CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        Self::load_cardinality_stats_from_tx(&tx, collection)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn list_cardinality_stats(
        &self,
    ) -> Result<std::collections::HashMap<String, CollectionCardinalityStats>, CassieError> {
        let entries = self.raw_scan_prefix(StorageFamily::Schema, &Self::cardinality_prefix())?;
        let prefix = Self::cardinality_prefix();
        let mut out = std::collections::HashMap::new();
        for (key, raw_value) in entries {
            let Ok(stats) = serde_json::from_slice::<CollectionCardinalityStats>(&raw_value) else {
                continue;
            };
            if let Some(collection) = key_encoding::utf8_suffix_after_prefix(&key, &prefix) {
                out.insert(collection, stats);
            }
        }
        Ok(out)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn save_cardinality_stats(
        &self,
        collection: &str,
        stats: &CollectionCardinalityStats,
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        Self::save_cardinality_stats_to_tx(&mut tx, collection, stats)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn delete_cardinality_stats(&self, collection: &str) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        tx.delete(Self::cardinality_key(collection))
            .map_err(CassieError::from)?;
        tx.commit(WriteOptions::sync()).map_err(CassieError::from)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn rebuild_cardinality_stats_for_collection(
        &self,
        collection: &str,
    ) -> Result<CollectionCardinalityStats, CassieError> {
        let documents = self.scan_documents(collection)?;
        let row_count = documents.len() as u64;
        let mut stats = CollectionCardinalityStats {
            row_count,
            hydrated: true,
            ..CollectionCardinalityStats::default()
        };

        if let Some(schema) = self.collection_schema(collection) {
            for field in schema.fields {
                stats.set_field_stats(
                    field.name.clone(),
                    super::cardinality_stats::field_cardinality_stats(&documents, &field.name),
                );
            }
        }

        for index in self.list_indexes()? {
            if index.collection != collection {
                continue;
            }
            let cardinality = documents
                .iter()
                .filter(|document| payload_contains_index_membership(&document.payload, &index))
                .count() as u64;
            stats.set_index_cardinality(
                CollectionCardinalityStats::index_key(&index.kind, &index.name),
                cardinality,
            );
        }

        for record in self.list_vector_indexes()? {
            if record.collection != collection {
                continue;
            }
            let cardinality = documents
                .iter()
                .filter(|document| payload_contains_vector_membership(&document.payload, &record))
                .count() as u64;
            stats.set_index_cardinality(
                CollectionCardinalityStats::vector_index_key(&record.field),
                cardinality,
            );
        }

        self.save_cardinality_stats(collection, &stats)?;
        Ok(stats)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn put_function(&self, metadata: &crate::catalog::FunctionMeta) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let key = Self::function_key(&metadata.name);
        let value =
            serde_json::to_vec(metadata).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(key, value, None).map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn get_function(
        &self,
        name: &str,
    ) -> Result<Option<crate::catalog::FunctionMeta>, CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        let raw = tx
            .get(&Self::function_key(name))
            .map_err(CassieError::from)?;
        let Some(raw) = raw else {
            return Ok(None);
        };

        serde_json::from_slice(&raw)
            .map(Some)
            .map_err(|error| CassieError::Parse(format!("invalid function metadata: {error}")))
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn list_functions(&self) -> Result<Vec<crate::catalog::FunctionMeta>, CassieError> {
        let entries = self.raw_scan_prefix(StorageFamily::Schema, &Self::function_prefix())?;
        let mut out: Vec<crate::catalog::FunctionMeta> = Vec::with_capacity(entries.len());
        for (_key, raw_value) in entries {
            let Ok(record) = serde_json::from_slice(&raw_value) else {
                continue;
            };
            out.push(record);
        }
        out.sort_by_key(|metadata| metadata.name.to_ascii_lowercase());
        Ok(out)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn delete_function(&self, name: &str) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        tx.delete(Self::function_key(name))
            .map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn put_procedure(
        &self,
        metadata: &crate::catalog::ProcedureMeta,
    ) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let key = Self::procedure_key(&metadata.name);
        let value =
            serde_json::to_vec(metadata).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(key, value, None).map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn get_procedure(
        &self,
        name: &str,
    ) -> Result<Option<crate::catalog::ProcedureMeta>, CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        let raw = tx
            .get(&Self::procedure_key(name))
            .map_err(CassieError::from)?;
        let Some(raw) = raw else {
            return Ok(None);
        };

        serde_json::from_slice(&raw)
            .map(Some)
            .map_err(|error| CassieError::Parse(format!("invalid procedure metadata: {error}")))
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn list_procedures(&self) -> Result<Vec<crate::catalog::ProcedureMeta>, CassieError> {
        let entries = self.raw_scan_prefix(StorageFamily::Schema, &Self::procedure_prefix())?;
        let mut out: Vec<crate::catalog::ProcedureMeta> = Vec::with_capacity(entries.len());
        for (_key, raw_value) in entries {
            let Ok(record) = serde_json::from_slice(&raw_value) else {
                continue;
            };
            out.push(record);
        }
        out.sort_by_key(|metadata| metadata.name.to_ascii_lowercase());
        Ok(out)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn delete_procedure(&self, name: &str) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        tx.delete(Self::procedure_key(name))
            .map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn put_view(&self, metadata: &crate::catalog::ViewMeta) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let key = Self::view_key(&metadata.name);
        let value =
            serde_json::to_vec(metadata).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(key, value, None).map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn get_view(&self, name: &str) -> Result<Option<crate::catalog::ViewMeta>, CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        let raw = tx.get(&Self::view_key(name)).map_err(CassieError::from)?;
        let Some(raw) = raw else {
            return Ok(None);
        };

        serde_json::from_slice(&raw)
            .map(Some)
            .map_err(|error| CassieError::Parse(format!("invalid view metadata: {error}")))
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn list_views(&self) -> Result<Vec<crate::catalog::ViewMeta>, CassieError> {
        let entries = self.raw_scan_prefix(StorageFamily::Schema, &Self::view_prefix())?;
        let mut out: Vec<crate::catalog::ViewMeta> = Vec::with_capacity(entries.len());
        for (_key, raw_value) in entries {
            let Ok(record) = serde_json::from_slice(&raw_value) else {
                continue;
            };
            out.push(record);
        }
        out.sort_by_key(|metadata| metadata.name.to_ascii_lowercase());
        Ok(out)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn delete_view(&self, name: &str) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        tx.delete(Self::view_key(name)).map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn put_role(&self, metadata: &RoleMeta) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        let key = Self::role_key(&metadata.name);
        let value =
            serde_json::to_vec(metadata).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(key, value, None).map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn get_role(&self, name: &str) -> Result<Option<RoleMeta>, CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        let raw = tx.get(&Self::role_key(name)).map_err(CassieError::from)?;
        let Some(raw) = raw else {
            return Ok(None);
        };

        serde_json::from_slice(&raw)
            .map(Some)
            .map_err(|error| CassieError::Parse(format!("invalid role metadata: {error}")))
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn list_roles(&self) -> Result<Vec<RoleMeta>, CassieError> {
        let entries = self.raw_scan_prefix(StorageFamily::Schema, &Self::role_prefix())?;
        let mut out: Vec<RoleMeta> = Vec::with_capacity(entries.len());
        for (_key, raw_value) in entries {
            let Ok(record) = serde_json::from_slice(&raw_value) else {
                continue;
            };
            out.push(record);
        }
        out.sort_by_key(|metadata| metadata.name.to_ascii_lowercase());
        Ok(out)
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn delete_role(&self, name: &str) -> Result<(), CassieError> {
        let mut tx = self.begin_schema_rw_tx()?;
        tx.delete(Self::role_key(name)).map_err(CassieError::from)?;
        tx.commit(cntryl_midge::WriteOptions::sync())
            .map_err(CassieError::from)?;
        Ok(())
    }

    pub fn collection_schema(&self, name: &str) -> Option<Schema> {
        let tx = self.begin_schema_readonly_tx().ok()?;
        if let Ok(Some(row_schema)) = Self::load_row_schema_from_tx(&tx, name) {
            return Some(row_schema.active_schema());
        }
        let raw = tx.get(&Self::collection_schema_key(name)).ok()??;
        serde_json::from_slice(&raw).ok()
    }

    /// # Errors
    ///
    /// Returns an error when validation, storage, or execution fails.
    pub fn projection_metadata(
        &self,
        collection: &str,
    ) -> Result<Option<ProjectionMeta>, CassieError> {
        let tx = self.begin_schema_readonly_tx()?;
        Self::load_projection_metadata_from_tx(&tx, collection)
    }

    pub fn list_collections(&self) -> Vec<String> {
        let Ok(tx) = self.begin_schema_readonly_tx() else {
            return Vec::new();
        };

        Self::load_collections(&tx).map_or_else(
            |_| Vec::new(),
            |mut values| {
                values.sort();
                values
            },
        )
    }

    pub fn list_collections_from_schema(&self) -> Vec<String> {
        let Ok(tx) = self.begin_schema_readonly_tx() else {
            return Vec::new();
        };
        let Ok(scan) = tx.scan(&Query::new().prefix(Self::schema_collection_prefix().into()))
        else {
            return Vec::new();
        };

        let mut collections = Vec::new();
        let prefix = Self::schema_collection_prefix();
        for (raw_key, _raw_value) in scan {
            if let Some(name) = key_encoding::utf8_suffix_after_prefix(&raw_key, &prefix) {
                collections.push(name);
            }
        }

        collections.sort();
        collections.dedup();
        collections
    }
}
