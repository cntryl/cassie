use std::collections::{HashMap, HashSet};

#[derive(Debug, Default)]
pub struct InvertedIndex {
    postings: HashMap<String, Vec<(String, usize)>>,
}

impl InvertedIndex {
    pub fn index_document(&mut self, doc_id: &str, tokens: &[String]) {
        self.remove_document(doc_id);

        let mut term_counts = HashMap::<String, usize>::new();
        for token in tokens {
            *term_counts.entry(token.to_string()).or_default() += 1;
        }

        for (token, frequency) in term_counts {
            self.postings
                .entry(token)
                .or_default()
                .push((doc_id.to_string(), frequency));
        }
    }

    pub fn index_term_counts(&mut self, doc_id: &str, term_counts: &HashMap<String, usize>) {
        self.remove_document(doc_id);

        for (token, frequency) in term_counts {
            self.postings
                .entry(token.clone())
                .or_default()
                .push((doc_id.to_string(), *frequency));
        }
    }

    pub fn postings(&self, token: &str) -> Option<&Vec<(String, usize)>> {
        self.postings.get(token)
    }

    pub fn candidate_documents(&self, tokens: &[String]) -> HashSet<String> {
        let mut candidates = HashSet::new();
        for token in tokens {
            if let Some(postings) = self.postings.get(token) {
                candidates.extend(postings.iter().map(|(doc_id, _)| doc_id.clone()));
            }
        }
        candidates
    }

    fn remove_document(&mut self, doc_id: &str) {
        self.postings.retain(|_, postings| {
            postings.retain(|(id, _)| id != doc_id);
            !postings.is_empty()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::InvertedIndex;
    use std::collections::{HashMap, HashSet};

    #[test]
    fn should_store_one_posting_per_document_with_term_frequency() {
        // Arrange
        let mut index = InvertedIndex::default();
        let tokens = vec![
            "alpha".to_string(),
            "bravo".to_string(),
            "alpha".to_string(),
        ];

        // Act
        index.index_document("doc-1", &tokens);

        // Assert
        assert_eq!(
            index.postings("alpha"),
            Some(&vec![("doc-1".to_string(), 2)])
        );
        assert_eq!(
            index.postings("bravo"),
            Some(&vec![("doc-1".to_string(), 1)])
        );
    }

    #[test]
    fn should_replace_existing_document_postings_when_reindexed() {
        // Arrange
        let mut index = InvertedIndex::default();
        index.index_document("doc-1", &["alpha".to_string()]);

        // Act
        index.index_document("doc-1", &["bravo".to_string(), "bravo".to_string()]);

        // Assert
        assert_eq!(index.postings("alpha"), None);
        assert_eq!(
            index.postings("bravo"),
            Some(&vec![("doc-1".to_string(), 2)])
        );
    }

    #[test]
    fn should_collect_unique_candidate_documents_for_query_terms() {
        // Arrange
        let mut index = InvertedIndex::default();
        index.index_term_counts(
            "doc-1",
            &HashMap::from([("alpha".to_string(), 2usize), ("bravo".to_string(), 1usize)]),
        );
        index.index_term_counts("doc-2", &HashMap::from([("bravo".to_string(), 3usize)]));
        index.index_term_counts("doc-3", &HashMap::from([("charlie".to_string(), 1usize)]));

        // Act
        let candidates = index.candidate_documents(&[
            "alpha".to_string(),
            "bravo".to_string(),
            "alpha".to_string(),
        ]);

        // Assert
        assert_eq!(
            candidates,
            HashSet::from(["doc-1".to_string(), "doc-2".to_string()])
        );
    }
}
