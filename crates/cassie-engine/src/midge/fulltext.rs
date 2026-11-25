//! Engine request wrappers for controlled native full-text reads.

use std::collections::BTreeSet;

use cassie_search::query::{CompiledQuery, ResolvedQuery};
use cassie_storage::adapter::fulltext_retrieval::{
    ControlledFulltextCandidateSet, ControlledRetrievalDocument, PersistedFulltextCandidateSet,
};
use cassie_storage::adapter::{DocumentRef, Midge};

use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

pub(crate) struct FulltextCandidates(ControlledFulltextCandidateSet);

impl FulltextCandidates {
    pub(crate) fn into_parts(self) -> (PersistedFulltextCandidateSet, QueryMemoryReservation) {
        let (candidates, reservation) = self.0.into_parts();
        (candidates, QueryMemoryReservation::from_core(reservation))
    }
}

pub(crate) struct RetrievalDocument(ControlledRetrievalDocument);

impl RetrievalDocument {
    pub(crate) fn into_parts(self) -> (DocumentRef, QueryMemoryReservation) {
        let (document, reservation) = self.0.into_parts();
        (document, QueryMemoryReservation::from_core(reservation))
    }
}

pub(crate) trait FulltextRequestReads {
    fn fulltext_candidate_set_controlled(
        &self,
        collection: &str,
        index_name: &str,
        terms: &[String],
        controls: &QueryExecutionControls,
    ) -> Result<FulltextCandidates, CassieError>;

    fn fulltext_candidate_set_for_ids_controlled(
        &self,
        collection: &str,
        index_name: &str,
        terms: &[String],
        allowed_ids: &BTreeSet<String>,
        controls: &QueryExecutionControls,
    ) -> Result<FulltextCandidates, CassieError>;

    fn fulltext_candidate_set_for_query_controlled(
        &self,
        collection: &str,
        index_name: &str,
        query: &CompiledQuery,
        controls: &QueryExecutionControls,
    ) -> Result<(FulltextCandidates, ResolvedQuery, QueryMemoryReservation), CassieError>;

    fn get_retrieval_document_controlled(
        &self,
        collection: &str,
        id: &str,
        controls: &QueryExecutionControls,
    ) -> Result<Option<RetrievalDocument>, CassieError>;
}

impl FulltextRequestReads for Midge {
    fn fulltext_candidate_set_controlled(
        &self,
        collection: &str,
        index_name: &str,
        terms: &[String],
        controls: &QueryExecutionControls,
    ) -> Result<FulltextCandidates, CassieError> {
        let candidates = self.fulltext_candidate_set_with_request(
            collection,
            index_name,
            terms,
            &controls.storage_request(),
        )?;
        Ok(FulltextCandidates(candidates))
    }

    fn fulltext_candidate_set_for_ids_controlled(
        &self,
        collection: &str,
        index_name: &str,
        terms: &[String],
        allowed_ids: &BTreeSet<String>,
        controls: &QueryExecutionControls,
    ) -> Result<FulltextCandidates, CassieError> {
        let candidates = self.fulltext_candidate_set_for_ids_with_request(
            collection,
            index_name,
            terms,
            allowed_ids,
            &controls.storage_request(),
        )?;
        Ok(FulltextCandidates(candidates))
    }

    fn fulltext_candidate_set_for_query_controlled(
        &self,
        collection: &str,
        index_name: &str,
        query: &CompiledQuery,
        controls: &QueryExecutionControls,
    ) -> Result<(FulltextCandidates, ResolvedQuery, QueryMemoryReservation), CassieError> {
        let (candidates, query, reservation) = self.fulltext_candidate_set_for_query_with_request(
            collection,
            index_name,
            query,
            &controls.storage_request(),
        )?;
        Ok((
            FulltextCandidates(candidates),
            query,
            QueryMemoryReservation::from_core(reservation),
        ))
    }

    fn get_retrieval_document_controlled(
        &self,
        collection: &str,
        id: &str,
        controls: &QueryExecutionControls,
    ) -> Result<Option<RetrievalDocument>, CassieError> {
        let document =
            self.get_retrieval_document_with_request(collection, id, &controls.storage_request())?;
        Ok(document.map(RetrievalDocument))
    }
}
