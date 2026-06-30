use super::{
    apply_feedback_observation, current_time_millis, prune_feedback_by_age, touch_feedback,
    OperatorFeedbackEstimate, RuntimeFeedbackKey, RuntimeFeedbackLookup,
    RuntimeFeedbackLookupState, RuntimeFeedbackObservation, RuntimeFeedbackRecord, RuntimeState,
    OPERATOR_FEEDBACK_CONFIDENCE_FLOOR_BPS, OPERATOR_FEEDBACK_MIN_STABLE_SAMPLES,
};

impl RuntimeState {
    pub fn feedback_lookup(&self, key: &RuntimeFeedbackKey) -> Option<RuntimeFeedbackRecord> {
        let lookup = self.feedback_lookup_state(key);
        match lookup.state {
            RuntimeFeedbackLookupState::Hit => {
                self.record_feedback_hit();
                lookup.record
            }
            RuntimeFeedbackLookupState::Missing | RuntimeFeedbackLookupState::Stale => {
                self.record_feedback_miss();
                None
            }
        }
    }

    pub(crate) fn feedback_lookup_state(&self, key: &RuntimeFeedbackKey) -> RuntimeFeedbackLookup {
        let now_ms = current_time_millis();
        let ttl_seconds = self.limits.feedback_ttl_seconds;
        let ttl_ms = ttl_seconds.saturating_mul(1_000);
        let mut feedback = self.feedback.lock().expect("runtime feedback");
        let mut evictions = 0;
        let record = feedback.entries.get(key).cloned();
        let lookup = match record {
            Some(record)
                if ttl_seconds > 0 && now_ms.saturating_sub(record.last_seen_ms) > ttl_ms =>
            {
                feedback.entries.remove(key);
                if let Some(position) = feedback.order.iter().position(|entry| entry == key) {
                    feedback.order.remove(position);
                }
                evictions = 1;
                RuntimeFeedbackLookup {
                    state: RuntimeFeedbackLookupState::Stale,
                    age_ms: now_ms.saturating_sub(record.last_seen_ms),
                    record: Some(record),
                }
            }
            Some(record) => {
                touch_feedback(&mut feedback.order, key);
                RuntimeFeedbackLookup {
                    state: RuntimeFeedbackLookupState::Hit,
                    age_ms: now_ms.saturating_sub(record.last_seen_ms),
                    record: Some(record),
                }
            }
            None => RuntimeFeedbackLookup {
                state: RuntimeFeedbackLookupState::Missing,
                age_ms: 0,
                record: None,
            },
        };
        drop(feedback);

        self.record_feedback_eviction(evictions);
        lookup
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn feedback_record(&self, key: &RuntimeFeedbackKey) -> Option<RuntimeFeedbackRecord> {
        self.feedback
            .lock()
            .expect("runtime feedback")
            .entries
            .get(key)
            .cloned()
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn record_feedback(
        &self,
        key: &RuntimeFeedbackKey,
        observation: &RuntimeFeedbackObservation,
    ) {
        let planner_feedback = self.limits.operator_feedback_enabled
            && matches!(key.operator_family.as_str(), "row_scan" | "index_read");
        let now_ms = current_time_millis();
        let max_entries = self.limits.feedback_entries.max(1);
        let mut feedback = self.feedback.lock().expect("runtime feedback");
        let mut evictions =
            prune_feedback_by_age(&mut feedback, now_ms, self.limits.feedback_ttl_seconds);

        let outlier_before = feedback
            .entries
            .get(key)
            .map(|record| record.outlier_samples)
            .unwrap_or_default();
        if let Some(record) = feedback.entries.get_mut(key) {
            apply_feedback_observation(record, observation, now_ms);
            touch_feedback(&mut feedback.order, key);
        } else {
            while feedback.entries.len() >= max_entries {
                let Some(oldest) = feedback.order.pop_front() else {
                    break;
                };
                if feedback.entries.remove(&oldest).is_some() {
                    evictions += 1;
                }
            }

            let mut record = RuntimeFeedbackRecord {
                first_seen_ms: now_ms,
                last_seen_ms: now_ms,
                ..RuntimeFeedbackRecord::default()
            };
            apply_feedback_observation(&mut record, observation, now_ms);
            feedback.entries.insert(key.clone(), record);
            feedback.order.push_back(key.clone());
        }
        let outlier_after = feedback
            .entries
            .get(key)
            .map(|record| record.outlier_samples)
            .unwrap_or_default();
        drop(feedback);

        if planner_feedback {
            self.bump_index_feedback_epoch();
        }
        self.record_feedback_write();
        self.record_feedback_eviction(evictions);
        if outlier_after > outlier_before {
            self.record_feedback_outlier();
        }
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn feedback_candidate_budget(&self, collection: &str) -> Option<usize> {
        let now_ms = current_time_millis();
        let mut feedback = self.feedback.lock().expect("runtime feedback");
        let evictions =
            prune_feedback_by_age(&mut feedback, now_ms, self.limits.feedback_ttl_seconds);
        let budget = feedback
            .entries
            .iter()
            .filter(|(key, record)| {
                key.collection.eq_ignore_ascii_case(collection)
                    && record.executions > 0
                    && record.candidate_count_total > 0
            })
            .map(|(_, record)| {
                record
                    .candidate_count_total
                    .saturating_add(record.executions - 1)
                    / record.executions
            })
            .max()
            .and_then(|value| usize::try_from(value).ok());
        drop(feedback);
        self.record_feedback_eviction(evictions);
        budget
    }

    pub(crate) fn operator_feedback_estimate(
        &self,
        key: &RuntimeFeedbackKey,
        base_cost: u64,
        estimated_rows: u64,
    ) -> OperatorFeedbackEstimate {
        if !self.limits.operator_feedback_enabled {
            return OperatorFeedbackEstimate::ignored("disabled", base_cost);
        }

        let lookup = self.feedback_lookup_state(key);
        match lookup.state {
            RuntimeFeedbackLookupState::Missing => {
                self.record_feedback_miss();
                OperatorFeedbackEstimate::ignored("missing", base_cost)
            }
            RuntimeFeedbackLookupState::Stale => {
                self.record_feedback_miss();
                OperatorFeedbackEstimate::ignored("stale", base_cost)
            }
            RuntimeFeedbackLookupState::Hit => {
                self.record_feedback_hit();
                let Some(record) = lookup.record else {
                    return OperatorFeedbackEstimate::ignored("missing", base_cost);
                };
                let freshness_bps =
                    freshness_confidence_bps(lookup.age_ms, self.limits.feedback_ttl_seconds);
                let confidence_bps = record.confidence_bps.min(freshness_bps);
                if confidence_bps < OPERATOR_FEEDBACK_CONFIDENCE_FLOOR_BPS
                    || record.stable_samples < OPERATOR_FEEDBACK_MIN_STABLE_SAMPLES
                {
                    return OperatorFeedbackEstimate {
                        state: "ignored",
                        reason: "low_confidence",
                        adjusted_cost: base_cost,
                        confidence_bps,
                        age_ms: lookup.age_ms,
                        samples: record.stable_samples,
                        outlier_samples: record.outlier_samples,
                    };
                }
                let adjusted_cost =
                    adjusted_cost_from_record(&record, base_cost, estimated_rows).max(1);
                OperatorFeedbackEstimate::used(
                    adjusted_cost,
                    confidence_bps,
                    lookup.age_ms,
                    record.stable_samples,
                    record.outlier_samples,
                )
            }
        }
    }

    pub(crate) fn record_operator_feedback_estimate(&self, estimate: &OperatorFeedbackEstimate) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        match estimate.reason {
            "applied" => metrics.feedback.used += 1,
            "disabled" => metrics.feedback.ignored_disabled += 1,
            "missing" => metrics.feedback.ignored_missing += 1,
            "stale" => metrics.feedback.ignored_stale += 1,
            "low_confidence" => metrics.feedback.ignored_low_confidence += 1,
            _ => {}
        }
    }

    pub(crate) fn replace_feedback_records(
        &self,
        records: Vec<(RuntimeFeedbackKey, RuntimeFeedbackRecord)>,
    ) {
        let mut feedback = self.feedback.lock().expect("runtime feedback");
        feedback.entries.clear();
        feedback.order.clear();
        for (key, record) in records {
            feedback.order.push_back(key.clone());
            feedback.entries.insert(key, record);
        }
    }

    pub(crate) fn feedback_records_for_persistence(
        &self,
    ) -> Vec<(RuntimeFeedbackKey, RuntimeFeedbackRecord)> {
        let feedback = self.feedback.lock().expect("runtime feedback");
        feedback
            .order
            .iter()
            .filter_map(|key| {
                feedback
                    .entries
                    .get(key)
                    .cloned()
                    .map(|record| (key.clone(), record))
            })
            .collect()
    }

    pub(crate) fn clear_feedback(&self) {
        let mut feedback = self.feedback.lock().expect("runtime feedback");
        feedback.entries.clear();
        feedback.order.clear();
    }

    /// # Panics
    ///
    /// Panics if an internal invariant required by this operation is violated.
    pub fn feedback_entry_count(&self) -> usize {
        self.feedback
            .lock()
            .expect("runtime feedback")
            .entries
            .len()
    }

    fn record_feedback_outlier(&self) {
        let mut metrics = self.metrics.lock().expect("runtime metrics");
        metrics.feedback.outliers += 1;
    }
}

fn freshness_confidence_bps(age_ms: u64, ttl_seconds: u64) -> u16 {
    if ttl_seconds == 0 {
        return 1_000;
    }

    let ttl_ms = ttl_seconds.saturating_mul(1_000).max(1);
    let remaining = ttl_ms.saturating_sub(age_ms).min(ttl_ms);
    u16::try_from(((remaining.saturating_mul(1_000)) / ttl_ms).max(1)).unwrap_or(1_000)
}

fn adjusted_cost_from_record(
    record: &RuntimeFeedbackRecord,
    base_cost: u64,
    estimated_rows: u64,
) -> u64 {
    let estimated_rows = estimated_rows.max(1);
    let rows_ratio_bps = ratio_bps(
        record
            .stable_average_rows_in()
            .max(record.stable_average_rows_out())
            .max(1),
        estimated_rows,
    );
    let read_ratio_bps = ratio_bps(record.stable_average_storage_reads().max(1), estimated_rows);
    let elapsed_ratio_bps = record
        .stable_average_elapsed_ms()
        .saturating_mul(100)
        .clamp(500, 4_000);
    let spill_penalty_bps = if record.spill_samples > 0 {
        1_250
    } else {
        1_000
    };
    let write_penalty_bps = if record.stable_average_storage_writes() > 0 {
        1_100
    } else {
        1_000
    };
    let combined_bps = ((elapsed_ratio_bps.saturating_mul(5))
        .saturating_add(read_ratio_bps.saturating_mul(3))
        .saturating_add(rows_ratio_bps.saturating_mul(2))
        / 10)
        .clamp(500, 4_000);
    base_cost
        .saturating_mul(combined_bps)
        .saturating_mul(spill_penalty_bps)
        .saturating_mul(write_penalty_bps)
        / 1_000
        / 1_000
        / 1_000
}

fn ratio_bps(observed: u64, baseline: u64) -> u64 {
    observed.saturating_mul(1_000) / baseline.max(1)
}
