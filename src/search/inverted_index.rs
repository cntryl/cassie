use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct InvertedIndex {
    postings: HashMap<String, Vec<(String, usize)>>,
}

impl InvertedIndex {
    pub fn index_document(&mut self, doc_id: &str, tokens: &[String]) {
        for token in tokens {
            let postings = self.postings.entry(token.to_string()).or_default();
            let freq = postings.iter().filter(|(id, _)| id == doc_id).count() + 1;
            postings.push((doc_id.to_string(), freq));
        }
    }

    pub fn postings(&self, token: &str) -> Option<&Vec<(String, usize)>> {
        self.postings.get(token)
    }
}
