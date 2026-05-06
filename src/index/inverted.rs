use crate::errors::Result;
use crate::index::{IndexPlugin, IndexStats};
use crate::store::RecordId;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Default)]
pub struct InvertedIndex {
    name: String,
    terms: BTreeMap<Vec<u8>, BTreeSet<RecordId>>,
    field_terms: BTreeMap<(String, Vec<u8>), BTreeSet<RecordId>>,
}

impl InvertedIndex {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            terms: BTreeMap::new(),
            field_terms: BTreeMap::new(),
        }
    }

    pub fn index_text(&mut self, text: &str, rid: RecordId) {
        for term in tokenize(text) {
            self.terms.entry(term).or_default().insert(rid);
        }
    }

    pub fn index_field_text(&mut self, field: impl Into<String>, text: &str, rid: RecordId) {
        let field = normalize(&field.into());
        for term in tokenize(text) {
            self.terms.entry(term.clone()).or_default().insert(rid);
            self.field_terms
                .entry((field.clone(), term))
                .or_default()
                .insert(rid);
        }
    }

    pub fn search(&self, term: &str) -> Vec<RecordId> {
        let term = normalize(term);
        self.terms
            .get(term.as_bytes())
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default()
    }

    pub fn search_field(&self, field: &str, term: &str) -> Vec<RecordId> {
        let key = (normalize(field), normalize(term).into_bytes());
        self.field_terms
            .get(&key)
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default()
    }

    pub fn postings(&self, term: &str) -> Vec<RecordId> {
        self.search(term)
    }

    pub fn field_postings(&self, field: &str, term: &str) -> Vec<RecordId> {
        self.search_field(field, term)
    }

    pub fn term_dictionary(&self) -> Vec<Vec<u8>> {
        self.terms.keys().cloned().collect()
    }
}

impl IndexPlugin for InvertedIndex {
    fn name(&self) -> &str {
        &self.name
    }

    fn put(&mut self, key: &[u8], rid: RecordId) -> Result<()> {
        self.terms
            .entry(normalize_bytes(key))
            .or_default()
            .insert(rid);
        Ok(())
    }

    fn get(&self, key: &[u8]) -> Option<RecordId> {
        self.terms
            .get(&normalize_bytes(key))
            .and_then(|set| set.iter().next().copied())
    }

    fn delete(&mut self, key: &[u8]) -> Result<()> {
        let term = normalize_bytes(key);
        self.terms.remove(&term);
        self.field_terms
            .retain(|(_, field_term), _| field_term != &term);
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }

    fn stats(&self) -> IndexStats {
        IndexStats {
            live_keys: self.terms.len() as u64,
            versions: self
                .terms
                .values()
                .chain(self.field_terms.values())
                .map(|set| set.len() as u64)
                .sum(),
        }
    }
}

fn tokenize(text: &str) -> impl Iterator<Item = Vec<u8>> + '_ {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(|part| normalize(part).into_bytes())
}

fn normalize(term: &str) -> String {
    term.to_ascii_lowercase()
}

fn normalize_bytes(term: &[u8]) -> Vec<u8> {
    String::from_utf8_lossy(term)
        .to_ascii_lowercase()
        .into_bytes()
}
