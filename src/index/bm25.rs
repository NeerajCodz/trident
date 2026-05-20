use crate::document::RecordId;
use std::cmp::Ordering;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default)]
pub struct Bm25Index {
    postings: BTreeMap<String, BTreeMap<RecordId, u32>>,
    doc_lengths: BTreeMap<RecordId, u32>,
    doc_count: u32,
    total_terms: u64,
}

impl Bm25Index {
    pub fn insert(&mut self, id: RecordId, text: &str) {
        self.remove(&id);
        let terms = tokenize(text);
        self.doc_count += 1;
        self.total_terms += terms.len() as u64;
        self.doc_lengths.insert(id.clone(), terms.len() as u32);
        let mut frequencies = BTreeMap::<String, u32>::new();
        for term in terms {
            *frequencies.entry(term).or_default() += 1;
        }
        for (term, frequency) in frequencies {
            self.postings
                .entry(term)
                .or_default()
                .insert(id.clone(), frequency);
        }
    }

    pub fn remove(&mut self, id: &RecordId) {
        let Some(length) = self.doc_lengths.remove(id) else {
            return;
        };
        self.doc_count = self.doc_count.saturating_sub(1);
        self.total_terms = self.total_terms.saturating_sub(length as u64);
        self.postings.retain(|_, posting| {
            posting.remove(id);
            !posting.is_empty()
        });
    }

    pub fn search(&self, query: &str) -> Vec<(RecordId, f32)> {
        let query_terms = tokenize(query);
        let doc_count = self.doc_count as f32;
        let avg_len = if self.doc_count == 0 {
            1.0
        } else {
            self.total_terms as f32 / doc_count
        };
        let mut scores = BTreeMap::<RecordId, f32>::new();
        for query_term in &query_terms {
            let Some(posting) = self.postings.get(query_term) else {
                continue;
            };
            let docs_with_term = posting.len() as f32;
            let idf = ((doc_count - docs_with_term + 0.5) / (docs_with_term + 0.5) + 1.0).ln();
            for (id, frequency) in posting {
                let frequency = *frequency as f32;
                let k1 = 1.2;
                let b = 0.75;
                let doc_len = *self.doc_lengths.get(id).unwrap_or(&0) as f32;
                let score = idf * (frequency * (k1 + 1.0))
                    / (frequency + k1 * (1.0 - b + b * doc_len / avg_len));
                *scores.entry(id.clone()).or_default() += score;
            }
        }
        let mut scores: Vec<_> = scores.into_iter().collect();
        scores.sort_by(|left, right| right.1.partial_cmp(&left.1).unwrap_or(Ordering::Equal));
        scores
    }

    pub fn posting_len(&self, term: &str) -> usize {
        self.postings
            .get(&term.to_ascii_lowercase())
            .map(BTreeMap::len)
            .unwrap_or_default()
    }

    pub fn doc_count(&self) -> u32 {
        self.doc_count
    }
}

fn tokenize(input: &str) -> Vec<String> {
    input
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(|term| term.to_ascii_lowercase())
        .collect()
}
