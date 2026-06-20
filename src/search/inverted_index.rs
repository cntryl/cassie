use std::collections::HashMap;

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

    pub fn postings(&self, token: &str) -> Option<&Vec<(String, usize)>> {
        self.postings.get(token)
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
}
