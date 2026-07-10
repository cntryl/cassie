use super::{
    binder, parser, Arc, Cassie, CassieError, CassieSession, QueryExecutionControls,
    RuntimeFeedbackKey, RuntimeFeedbackObservation,
};

impl Cassie {
    fn binding_context_for_session(
        &self,
        session: Option<&CassieSession>,
    ) -> binder::BindingContext {
        let database = session
            .and_then(CassieSession::current_database)
            .unwrap_or(self.default_database.as_str());
        let search_path = session.map_or_else(
            || vec![crate::catalog::DEFAULT_SCHEMA.to_string()],
            CassieSession::search_path,
        );
        if self.database_catalog_enforced() {
            binder::BindingContext::scoped(database.to_string(), search_path)
        } else {
            binder::BindingContext::unscoped(database.to_string(), search_path)
        }
    }

    pub(crate) fn feedback_keys_for_plan(
        &self,
        database: Option<&str>,
        search_path: &[String],
        physical: &crate::planner::physical::PhysicalPlan,
    ) -> Vec<RuntimeFeedbackKey> {
        let schema_epoch = self.runtime.schema_epoch();
        let mut keys = Vec::new();
        if let Some(index_name) = physical.read.selected_index.as_deref() {
            let index = self.catalog.get_index(&physical.collection, index_name);
            keys.push(crate::runtime::normalized_feedback_key(
                database.map(str::to_string),
                search_path.to_owned(),
                schema_epoch,
                &physical.collection,
                "index_read",
                &physical.logical,
                index.as_ref(),
            ));
        } else {
            keys.push(crate::runtime::normalized_feedback_key(
                database.map(str::to_string),
                search_path.to_owned(),
                schema_epoch,
                &physical.collection,
                "row_scan",
                &physical.logical,
                None,
            ));
        }

        for (operator, family) in [
            (
                crate::planner::physical::Operator::VectorSearch,
                "vector_search",
            ),
            (
                crate::planner::physical::Operator::FullTextSearch,
                "fulltext_search",
            ),
            (crate::planner::physical::Operator::Join, "join"),
            (crate::planner::physical::Operator::Aggregate, "aggregate"),
        ] {
            if physical.operators.contains(&operator) {
                keys.push(crate::runtime::normalized_feedback_key(
                    database.map(str::to_string),
                    search_path.to_owned(),
                    schema_epoch,
                    &physical.collection,
                    family,
                    &physical.logical,
                    None,
                ));
            }
        }

        keys
    }

    pub(crate) fn observe_feedback_lookup(&self, keys: &[RuntimeFeedbackKey]) {
        for key in keys {
            let _ = self.runtime.feedback_lookup(key);
        }
    }

    pub(crate) fn persist_runtime_feedback(&self) {
        let records = self.runtime.feedback_records_for_persistence();
        let persisted = self.midge.replace_runtime_feedback_records(&records);
        self.runtime
            .record_storage_access("schema", true, persisted.is_ok());
    }

    pub(crate) fn record_feedback_for_keys(
        &self,
        keys: Vec<RuntimeFeedbackKey>,
        observation: &RuntimeFeedbackObservation,
    ) {
        for key in keys {
            self.runtime.record_feedback(&key, observation);
        }
        self.persist_runtime_feedback();
    }

    fn feedback_planned_candidate(
        &self,
        database: Option<&str>,
        search_path: Vec<String>,
        collection: &str,
        logical: &crate::planner::logical::LogicalPlan,
        candidate: &crate::planner::physical::ReadOperatorCandidate,
    ) -> (RuntimeFeedbackKey, crate::runtime::OperatorFeedbackEstimate) {
        let key = crate::runtime::normalized_feedback_key(
            database.map(str::to_string),
            search_path,
            self.runtime.schema_epoch(),
            collection,
            candidate.operator_family,
            logical,
            candidate.index.as_ref(),
        );
        let estimate = self.runtime.operator_feedback_estimate(
            &key,
            candidate.base_cost,
            candidate.estimated_rows,
        );
        self.runtime.record_operator_feedback_estimate(&estimate);
        (key, estimate)
    }

    fn select_operator_feedback_plan(
        &self,
        database: Option<&str>,
        search_path: &[String],
        collection: &str,
        logical: &crate::planner::logical::LogicalPlan,
        selection: &crate::planner::physical::ReadOperatorSelection,
    ) -> (
        Option<String>,
        crate::planner::physical::OperatorFeedbackPlanDiagnostics,
    ) {
        let Some(base_candidate) = selection
            .candidates
            .iter()
            .find(|candidate| candidate.base_selected)
            .or_else(|| selection.candidates.first())
        else {
            return (
                selection.base_selected_index.clone(),
                crate::planner::physical::OperatorFeedbackPlanDiagnostics::default(),
            );
        };

        let (_base_key, base_estimate) = self.feedback_planned_candidate(
            database,
            search_path.to_owned(),
            collection,
            logical,
            base_candidate,
        );
        let mut chosen_candidate = base_candidate;
        let mut chosen_estimate = base_estimate.clone();
        let mut chosen_cost = if base_estimate.state == "used" {
            base_estimate.adjusted_cost
        } else {
            base_candidate.base_cost
        };

        for candidate in selection
            .candidates
            .iter()
            .filter(|candidate| !candidate.base_selected)
        {
            let (_key, estimate) = self.feedback_planned_candidate(
                database,
                search_path.to_owned(),
                collection,
                logical,
                candidate,
            );
            if estimate.state == "used" && estimate.adjusted_cost < chosen_cost {
                chosen_candidate = candidate;
                chosen_cost = estimate.adjusted_cost;
                chosen_estimate = estimate;
            }
        }

        let diagnostics = if chosen_estimate.state == "used" {
            crate::planner::physical::OperatorFeedbackPlanDiagnostics {
                state: "used".to_string(),
                reason: chosen_estimate.reason.to_string(),
                base_candidate: base_candidate.label.clone(),
                selected_candidate: chosen_candidate.label.clone(),
                base_selected_cost: base_candidate.base_cost,
                adjusted_selected_cost: chosen_cost,
                confidence_bps: chosen_estimate.confidence_bps,
                age_ms: chosen_estimate.age_ms,
                samples: chosen_estimate.samples,
                outlier_samples: chosen_estimate.outlier_samples,
            }
        } else {
            crate::planner::physical::OperatorFeedbackPlanDiagnostics {
                state: "ignored".to_string(),
                reason: base_estimate.reason.to_string(),
                base_candidate: base_candidate.label.clone(),
                selected_candidate: base_candidate.label.clone(),
                base_selected_cost: base_candidate.base_cost,
                adjusted_selected_cost: base_candidate.base_cost,
                confidence_bps: base_estimate.confidence_bps,
                age_ms: base_estimate.age_ms,
                samples: base_estimate.samples,
                outlier_samples: base_estimate.outlier_samples,
            }
        };

        (chosen_candidate.selected_index.clone(), diagnostics)
    }

    pub(crate) fn compile_physical_plan(
        &self,
        parsed: crate::sql::ast::ParsedStatement,
        session: Option<&CassieSession>,
        controls: Option<&QueryExecutionControls>,
    ) -> Result<Arc<crate::planner::physical::PhysicalPlan>, CassieError> {
        let context = self.binding_context_for_session(session);
        let bound = binder::bind_with_context(parsed, &self.catalog, &context)?;
        if controls.is_some_and(QueryExecutionControls::is_timed_out) {
            return Err(CassieError::DeadlineExceeded);
        }

        let logical = crate::planner::logical::plan(&bound)?;
        let optimized = crate::planner::optimizer::optimize(logical);
        let cardinality_stats = self.catalog.cardinality_snapshot();
        let selection = crate::planner::physical::read_operator_selection(
            &optimized,
            bound.indexes.as_slice(),
            &cardinality_stats,
        );
        let (operator_selected_index, operator_feedback) = self.select_operator_feedback_plan(
            session.and_then(CassieSession::current_database),
            &context.search_path,
            &optimized.collection,
            &optimized,
            &selection,
        );
        let limits = self.runtime.limits();
        let (selected_index, adaptive_plan) =
            crate::planner::physical::select_adaptive_read_operator(
                &selection,
                operator_selected_index,
                &operator_feedback,
                &limits,
            );

        let mut physical = crate::planner::physical::build_with_selection(
            optimized,
            bound.indexes.as_slice(),
            &cardinality_stats,
            selected_index,
            operator_feedback,
            adaptive_plan,
        );
        physical.collection_schema = self.catalog.get_schema(&physical.logical.collection);
        Ok(Arc::new(physical))
    }

    #[doc(hidden)]
    #[must_use]
    pub fn feedback_record_for_diagnostics(
        &self,
        key: &RuntimeFeedbackKey,
    ) -> Option<crate::runtime::RuntimeFeedbackRecord> {
        self.runtime.feedback_record(key)
    }

    #[doc(hidden)]
    pub fn read_operator_feedback_key_for_diagnostics(
        &self,
        session: &CassieSession,
        sql: &str,
        candidate_index: Option<&str>,
    ) -> Result<RuntimeFeedbackKey, CassieError> {
        let parsed = parser::parse_statement(sql)?;
        let context = self.binding_context_for_session(Some(session));
        let bound = binder::bind_with_context(parsed, &self.catalog, &context)?;
        let logical = crate::planner::optimizer::optimize(crate::planner::logical::plan(&bound)?);
        let cardinality_stats = self.catalog.cardinality_snapshot();
        let selection = crate::planner::physical::read_operator_selection(
            &logical,
            bound.indexes.as_slice(),
            &cardinality_stats,
        );
        let candidate = selection
            .candidates
            .iter()
            .find(|candidate| match candidate_index {
                Some(index_name) => candidate.selected_index.as_deref() == Some(index_name),
                None => candidate.selected_index.is_none(),
            })
            .ok_or_else(|| {
                CassieError::Planner("operator feedback candidate not available".to_string())
            })?;
        Ok(crate::runtime::normalized_feedback_key(
            session.database.clone(),
            session.search_path(),
            self.runtime.schema_epoch(),
            &logical.collection,
            candidate.operator_family,
            &logical,
            candidate.index.as_ref(),
        ))
    }

    #[doc(hidden)]
    pub fn seed_feedback_for_diagnostics(
        &self,
        key: &RuntimeFeedbackKey,
        observation: &RuntimeFeedbackObservation,
    ) -> Result<(), CassieError> {
        self.runtime.record_feedback(key, observation);
        self.persist_runtime_feedback();
        Ok(())
    }

    #[doc(hidden)]
    pub fn clear_feedback_for_diagnostics(&self) {
        self.runtime.clear_feedback();
    }

    #[doc(hidden)]
    pub fn reload_feedback_from_storage_for_diagnostics(&self) -> Result<(), CassieError> {
        self.hydrate_runtime_feedback()
    }
}
