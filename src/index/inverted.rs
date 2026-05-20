use crate::store::RecordId;
use crate::errors::{Result, TridentError};
use crate::index::{IndexPlugin, IndexStats};
use crate::io::{BinaryReader, BinaryWriter, crc32c};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const INVERTED_SNAPSHOT_MAGIC: u32 = 0x494E_5632; // "INV2"
const INVERTED_SNAPSHOT_VERSION: u8 = 1;

// BM25 defaults
const BM25_K1: f32 = 1.2;
const BM25_B: f32 = 0.75;

/// Inverted index with BM25 scoring, field-level postings, and binary snapshot persistence.
#[derive(Debug)]
pub struct InvertedIndex {
    name: String,
    dir: PathBuf,
    // Term -> set of RecordIds (global postings list)
    terms: BTreeMap<Vec<u8>, BTreeSet<RecordId>>,
    // (field_name, term) -> set of RecordIds (field-level postings)
    field_terms: BTreeMap<(String, Vec<u8>), BTreeSet<RecordId>>,
    // BM25 data: RecordId -> (term -> frequency)
    term_frequencies: BTreeMap<RecordId, BTreeMap<Vec<u8>, u32>>,
    // BM25 data: RecordId -> document length (number of tokens)
    doc_lengths: BTreeMap<RecordId, u32>,
    // Number of indexed documents
    doc_count: u32,
    // Total terms across all documents
    total_terms: u64,
}

impl InvertedIndex {
    /// Create a new in-memory inverted index (no persistence).
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            dir: PathBuf::new(),
            terms: BTreeMap::new(),
            field_terms: BTreeMap::new(),
            term_frequencies: BTreeMap::new(),
            doc_lengths: BTreeMap::new(),
            doc_count: 0,
            total_terms: 0,
        }
    }

    /// Open or create a persistent inverted index at `dir`.
    /// If a snapshot file exists, it is loaded; otherwise a fresh index is created.
    pub fn open(name: impl Into<String>, dir: impl Into<PathBuf>) -> Result<Self> {
        let name = name.into();
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;

        let path = Self::snapshot_path(&dir, &name);
        if path.exists() {
            let bytes = std::fs::read(&path)?;
            let on_disk = decode_binary_snapshot(&bytes, &path)?;
            Ok(Self {
                name,
                dir,
                terms: on_disk.terms,
                field_terms: on_disk.field_terms,
                term_frequencies: on_disk.term_frequencies,
                doc_lengths: on_disk.doc_lengths,
                doc_count: on_disk.doc_count,
                total_terms: on_disk.total_terms,
            })
        } else {
            Ok(Self {
                name,
                dir,
                terms: BTreeMap::new(),
                field_terms: BTreeMap::new(),
                term_frequencies: BTreeMap::new(),
                doc_lengths: BTreeMap::new(),
                doc_count: 0,
                total_terms: 0,
            })
        }
    }

    fn snapshot_path(dir: &Path, name: &str) -> PathBuf {
        dir.join(format!("{name}.invidx"))
    }

    /// Index free text under a RecordId (global postings + BM25 tracking).
    pub fn index_text(&mut self, text: &str, rid: RecordId) {
        // Remove existing BM25 data for this rid first
        self.remove_bm25(&rid);

        let tokens: Vec<Vec<u8>> = tokenize(text).collect();
        self.doc_count += 1;
        self.total_terms += tokens.len() as u64;
        self.doc_lengths.insert(rid, tokens.len() as u32);

        // Count term frequencies for this document
        let mut freq_map: BTreeMap<Vec<u8>, u32> = BTreeMap::new();
        for term in &tokens {
            *freq_map.entry(term.clone()).or_default() += 1;
        }

        // Insert into global postings and term frequency tracking
        for (term, freq) in &freq_map {
            self.terms.entry(term.clone()).or_default().insert(rid);
            self.term_frequencies
                .entry(rid)
                .or_default()
                .insert(term.clone(), *freq);
        }
    }

    /// Index text under a specific field name and RecordId.
    /// Adds to both global and field-level postings, plus BM25 tracking.
    pub fn index_field_text(&mut self, field: impl Into<String>, text: &str, rid: RecordId) {
        let field = normalize(&field.into());
        // Remove existing BM25 data for this rid first
        self.remove_bm25(&rid);

        let tokens: Vec<Vec<u8>> = tokenize(text).collect();
        self.doc_count += 1;
        self.total_terms += tokens.len() as u64;
        self.doc_lengths.insert(rid, tokens.len() as u32);

        let mut freq_map: BTreeMap<Vec<u8>, u32> = BTreeMap::new();
        for term in &tokens {
            *freq_map.entry(term.clone()).or_default() += 1;
        }

        for (term, freq) in &freq_map {
            self.terms.entry(term.clone()).or_default().insert(rid);
            self.field_terms
                .entry((field.clone(), term.clone()))
                .or_default()
                .insert(rid);
            self.term_frequencies
                .entry(rid)
                .or_default()
                .insert(term.clone(), *freq);
        }
    }

    /// Remove BM25 tracking data for a record (used before re-indexing).
    fn remove_bm25(&mut self, rid: &RecordId) {
        if let Some(old_len) = self.doc_lengths.remove(rid) {
            self.doc_count = self.doc_count.saturating_sub(1);
            self.total_terms = self.total_terms.saturating_sub(old_len as u64);
        }
        self.term_frequencies.remove(rid);
    }

    /// Search globally and return results ranked by BM25 score.
    pub fn search_ranked(&self, query: &str) -> Vec<(RecordId, f32)> {
        let query_terms: Vec<String> = tokenize_string(query);
        let doc_count = self.doc_count as f32;
        let avg_len = if self.doc_count == 0 {
            1.0
        } else {
            self.total_terms as f32 / doc_count
        };

        let mut scores: BTreeMap<RecordId, f32> = BTreeMap::new();

        for query_term in &query_terms {
            let term_bytes = query_term.as_bytes().to_vec();
            let Some(posting) = self.terms.get(&term_bytes) else {
                continue;
            };
            let docs_with_term = posting.len() as f32;
            let idf =
                ((doc_count - docs_with_term + 0.5) / (docs_with_term + 0.5) + 1.0).ln();

            for rid in posting {
                let frequency = self
                    .term_frequencies
                    .get(rid)
                    .and_then(|m| m.get(&term_bytes))
                    .copied()
                    .unwrap_or(0) as f32;
                let doc_len = self.doc_lengths.get(rid).copied().unwrap_or(0) as f32;
                let tf_norm = (frequency * (BM25_K1 + 1.0))
                    / (frequency + BM25_K1 * (1.0 - BM25_B + BM25_B * doc_len / avg_len));
                *scores.entry(*rid).or_default() += idf * tf_norm;
            }
        }

        let mut results: Vec<(RecordId, f32)> = scores.into_iter().collect();
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results
    }

    /// Search within a specific field, returning results ranked by BM25 score.
    pub fn search_field_ranked(&self, field: &str, query: &str) -> Vec<(RecordId, f32)> {
        let field = normalize(field);
        let query_terms: Vec<String> = tokenize_string(query);
        let doc_count = self.doc_count as f32;
        let avg_len = if self.doc_count == 0 {
            1.0
        } else {
            self.total_terms as f32 / doc_count
        };

        let mut scores: BTreeMap<RecordId, f32> = BTreeMap::new();

        for query_term in &query_terms {
            let term_bytes = query_term.as_bytes().to_vec();
            let key = (field.clone(), term_bytes.clone());
            let Some(posting) = self.field_terms.get(&key) else {
                continue;
            };
            let docs_with_term = posting.len() as f32;
            let idf =
                ((doc_count - docs_with_term + 0.5) / (docs_with_term + 0.5) + 1.0).ln();

            for rid in posting {
                let frequency = self
                    .term_frequencies
                    .get(rid)
                    .and_then(|m| m.get(&term_bytes))
                    .copied()
                    .unwrap_or(0) as f32;
                let doc_len = self.doc_lengths.get(rid).copied().unwrap_or(0) as f32;
                let tf_norm = (frequency * (BM25_K1 + 1.0))
                    / (frequency + BM25_K1 * (1.0 - BM25_B + BM25_B * doc_len / avg_len));
                *scores.entry(*rid).or_default() += idf * tf_norm;
            }
        }

        let mut results: Vec<(RecordId, f32)> = scores.into_iter().collect();
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results
    }

    /// Simple unranked global search (returns matching RecordIds).
    pub fn search(&self, term: &str) -> Vec<RecordId> {
        let term = normalize(term);
        self.terms
            .get(term.as_bytes())
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Simple unranked field-level search (returns matching RecordIds).
    pub fn search_field(&self, field: &str, term: &str) -> Vec<RecordId> {
        let key = (normalize(field), normalize(term).into_bytes());
        self.field_terms
            .get(&key)
            .map(|set| set.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Return postings list for a term (alias for search).
    pub fn postings(&self, term: &str) -> Vec<RecordId> {
        self.search(term)
    }

    /// Return field-level postings for a (field, term) pair.
    pub fn field_postings(&self, field: &str, term: &str) -> Vec<RecordId> {
        self.search_field(field, term)
    }

    /// Return all terms in the term dictionary.
    pub fn term_dictionary(&self) -> Vec<Vec<u8>> {
        self.terms.keys().cloned().collect()
    }

    /// Return the document frequency for a term.
    pub fn document_frequency(&self, term: &str) -> usize {
        let term = normalize(term);
        self.terms
            .get(term.as_bytes())
            .map(BTreeSet::len)
            .unwrap_or(0)
    }

    /// Return BM25 stats.
    pub fn bm25_stats(&self) -> (u32, u64, f32) {
        let avg_len = if self.doc_count == 0 {
            0.0
        } else {
            self.total_terms as f32 / self.doc_count as f32
        };
        (self.doc_count, self.total_terms, avg_len)
    }
}

impl IndexPlugin for InvertedIndex {
    fn name(&self) -> &str {
        &self.name
    }

    fn put(&mut self, key: &[u8], rid: RecordId) -> Result<()> {
        let term = normalize_bytes(key);
        self.terms.entry(term).or_default().insert(rid);
        Ok(())
    }

    fn get(&self, key: &[u8]) -> Option<RecordId> {
        self.terms
            .get(&normalize_bytes(key))
            .and_then(|set| set.iter().next().cloned())
    }

    fn delete(&mut self, key: &[u8]) -> Result<()> {
        let term = normalize_bytes(key);
        // Collect rids first to avoid borrow conflict with remove_bm25
        let rids: Vec<RecordId> = self
            .terms
            .get(&term)
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default();
        for rid in rids {
            self.remove_bm25(&rid);
        }
        self.terms.remove(&term);
        // Remove from field-level postings
        self.field_terms
            .retain(|(_, field_term), _| field_term != &term);
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        if self.dir.as_os_str().is_empty() {
            return Ok(()); // No persistence dir configured
        }
        let on_disk = OnDisk {
            terms: self.terms.clone(),
            field_terms: self.field_terms.clone(),
            term_frequencies: self.term_frequencies.clone(),
            doc_lengths: self.doc_lengths.clone(),
            doc_count: self.doc_count,
            total_terms: self.total_terms,
        };
        let bytes = encode_binary_snapshot(&on_disk);
        let path = Self::snapshot_path(&self.dir, &self.name);
        std::fs::write(&path, bytes)?;
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

// --- Binary snapshot format ---

struct OnDisk {
    terms: BTreeMap<Vec<u8>, BTreeSet<RecordId>>,
    field_terms: BTreeMap<(String, Vec<u8>), BTreeSet<RecordId>>,
    term_frequencies: BTreeMap<RecordId, BTreeMap<Vec<u8>, u32>>,
    doc_lengths: BTreeMap<RecordId, u32>,
    doc_count: u32,
    total_terms: u64,
}

fn encode_binary_snapshot(on_disk: &OnDisk) -> Vec<u8> {
    let mut payload = BinaryWriter::new();

    // Scalar stats
    payload.write_u32(on_disk.doc_count);
    payload.write_u64(on_disk.total_terms);

    // Global terms: term -> [rid...]
    payload.write_u32(on_disk.terms.len() as u32);
    for (term, rids) in &on_disk.terms {
        payload.write_len_bytes(term);
        payload.write_u32(rids.len() as u32);
        for rid in rids {
            payload.write_u64(rid.0);
        }
    }

    // Field terms: (field, term) -> [rid...]
    payload.write_u32(on_disk.field_terms.len() as u32);
    for ((field, term), rids) in &on_disk.field_terms {
        payload.write_len_bytes(field.as_bytes());
        payload.write_len_bytes(term);
        payload.write_u32(rids.len() as u32);
        for rid in rids {
            payload.write_u64(rid.0);
        }
    }

    // Term frequencies: rid -> (term -> freq)
    payload.write_u32(on_disk.term_frequencies.len() as u32);
    for (rid, freq_map) in &on_disk.term_frequencies {
        payload.write_u64(rid.0);
        payload.write_u32(freq_map.len() as u32);
        for (term, freq) in freq_map {
            payload.write_len_bytes(term);
            payload.write_u32(*freq);
        }
    }

    // Doc lengths: rid -> length
    payload.write_u32(on_disk.doc_lengths.len() as u32);
    for (rid, len) in &on_disk.doc_lengths {
        payload.write_u64(rid.0);
        payload.write_u32(*len);
    }

    let payload = payload.into_inner();
    let mut out = BinaryWriter::new();
    out.write_u32(INVERTED_SNAPSHOT_MAGIC);
    out.write_u8(INVERTED_SNAPSHOT_VERSION);
    out.write_u32(payload.len() as u32);
    out.write_u32(crc32c(&payload));
    out.write_bytes(&payload);
    out.into_inner()
}

fn decode_binary_snapshot(bytes: &[u8], source: &Path) -> Result<OnDisk> {
    if bytes.len() < 13 {
        return Err(TridentError::Corrupt {
            path: source.to_path_buf(),
            reason: "truncated inverted index snapshot header".to_string(),
        });
    }
    let magic = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    if magic != INVERTED_SNAPSHOT_MAGIC {
        return Err(TridentError::Corrupt {
            path: source.to_path_buf(),
            reason: "bad inverted index snapshot magic".to_string(),
        });
    }
    if bytes[4] != INVERTED_SNAPSHOT_VERSION {
        return Err(TridentError::Corrupt {
            path: source.to_path_buf(),
            reason: format!(
                "unsupported inverted index snapshot version {}",
                bytes[4]
            ),
        });
    }
    let payload_len = u32::from_le_bytes([bytes[5], bytes[6], bytes[7], bytes[8]]) as usize;
    let expected_crc = u32::from_le_bytes([bytes[9], bytes[10], bytes[11], bytes[12]]);
    if bytes.len() < 13 + payload_len {
        return Err(TridentError::Corrupt {
            path: source.to_path_buf(),
            reason: "truncated inverted index snapshot payload".to_string(),
        });
    }
    let payload = &bytes[13..13 + payload_len];
    let actual_crc = crc32c(payload);
    if actual_crc != expected_crc {
        return Err(TridentError::Corrupt {
            path: source.to_path_buf(),
            reason: format!(
                "inverted index snapshot checksum mismatch: expected {expected_crc:#010x}, got {actual_crc:#010x}"
            ),
        });
    }

    let mut reader = BinaryReader::new(payload, source);

    // Scalar stats
    let doc_count = reader.read_u32()?;
    let total_terms = reader.read_u64()?;

    // Global terms
    let term_count = reader.read_u32()? as usize;
    let mut terms: BTreeMap<Vec<u8>, BTreeSet<RecordId>> = BTreeMap::new();
    for _ in 0..term_count {
        let term = reader.read_len_bytes()?;
        let rid_count = reader.read_u32()? as usize;
        let mut rids = BTreeSet::new();
        for _ in 0..rid_count {
            rids.insert(RecordId(reader.read_u64()?));
        }
        terms.insert(term, rids);
    }

    // Field terms
    let field_term_count = reader.read_u32()? as usize;
    let mut field_terms: BTreeMap<(String, Vec<u8>), BTreeSet<RecordId>> = BTreeMap::new();
    for _ in 0..field_term_count {
        let field_bytes = reader.read_len_bytes()?;
        let field = String::from_utf8_lossy(&field_bytes).into_owned();
        let term = reader.read_len_bytes()?;
        let rid_count = reader.read_u32()? as usize;
        let mut rids = BTreeSet::new();
        for _ in 0..rid_count {
            rids.insert(RecordId(reader.read_u64()?));
        }
        field_terms.insert((field, term), rids);
    }

    // Term frequencies
    let tf_count = reader.read_u32()? as usize;
    let mut term_frequencies: BTreeMap<RecordId, BTreeMap<Vec<u8>, u32>> = BTreeMap::new();
    for _ in 0..tf_count {
        let rid = RecordId(reader.read_u64()?);
        let freq_count = reader.read_u32()? as usize;
        let mut freq_map = BTreeMap::new();
        for _ in 0..freq_count {
            let term = reader.read_len_bytes()?;
            let freq = reader.read_u32()?;
            freq_map.insert(term, freq);
        }
        term_frequencies.insert(rid, freq_map);
    }

    // Doc lengths
    let dl_count = reader.read_u32()? as usize;
    let mut doc_lengths: BTreeMap<RecordId, u32> = BTreeMap::new();
    for _ in 0..dl_count {
        let rid = RecordId(reader.read_u64()?);
        let len = reader.read_u32()?;
        doc_lengths.insert(rid, len);
    }

    Ok(OnDisk {
        terms,
        field_terms,
        term_frequencies,
        doc_lengths,
        doc_count,
        total_terms,
    })
}

// --- Tokenizer helpers ---

fn tokenize(text: &str) -> impl Iterator<Item = Vec<u8>> + '_ {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(|part| normalize(part).into_bytes())
}

fn tokenize_string(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(normalize)
        .collect()
}

fn normalize(term: &str) -> String {
    term.to_ascii_lowercase()
}

fn normalize_bytes(term: &[u8]) -> Vec<u8> {
    String::from_utf8_lossy(term)
        .to_ascii_lowercase()
        .into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn rid(n: u64) -> RecordId {
        RecordId(n)
    }

    #[test]
    fn test_basic_index_and_search() {
        let mut idx = InvertedIndex::new("test");
        idx.index_text("hello world", rid(1));
        idx.index_text("hello rust", rid(2));
        idx.index_text("goodbye world", rid(3));

        let results = idx.search("hello");
        assert_eq!(results.len(), 2);
        assert!(results.contains(&rid(1)));
        assert!(results.contains(&rid(2)));

        let results = idx.search("world");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_field_search() {
        let mut idx = InvertedIndex::new("test");
        idx.index_field_text("title", "hello world", rid(1));
        idx.index_field_text("body", "hello rust", rid(2));

        let results = idx.search_field("title", "hello");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], rid(1));

        let results = idx.search_field("body", "rust");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], rid(2));
    }

    #[test]
    fn test_bm25_ranking() {
        let mut idx = InvertedIndex::new("test");
        idx.index_text("rust programming language", rid(1));
        idx.index_text("rust systems programming", rid(2));
        idx.index_text("python scripting", rid(3));

        let results = idx.search_ranked("rust");
        assert_eq!(results.len(), 2);
        // Both contain "rust" once, similar doc lengths -> similar scores
        assert!(results[0].1 > 0.0);
        assert!(results[1].1 > 0.0);
    }

    #[test]
    fn test_bm25_field_ranking() {
        let mut idx = InvertedIndex::new("test");
        idx.index_field_text("title", "rust guide", rid(1));
        idx.index_field_text("body", "rust is great for systems", rid(2));

        let results = idx.search_field_ranked("title", "rust");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, rid(1));
    }

    #[test]
    fn test_binary_snapshot_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path();

        // Write index
        {
            let mut idx = InvertedIndex::open("snap", dir).unwrap();
            idx.index_text("hello world", rid(1));
            idx.index_field_text("title", "rust lang", rid(2));
            idx.index_text("goodbye", rid(3));
            idx.flush().unwrap();
        }

        // Read back
        {
            let idx = InvertedIndex::open("snap", dir).unwrap();
            assert_eq!(idx.doc_count, 3);
            let results = idx.search("hello");
            assert_eq!(results.len(), 1);
            assert_eq!(results[0], rid(1));
            let results = idx.search_field("title", "rust");
            assert_eq!(results.len(), 1);
            assert_eq!(results[0], rid(2));
            let ranked = idx.search_ranked("hello");
            assert_eq!(ranked.len(), 1);
            assert!(ranked[0].1 > 0.0);
        }
    }

    #[test]
    fn test_no_persistence_without_dir() {
        let mut idx = InvertedIndex::new("mem");
        idx.index_text("test", rid(1));
        // flush() on a non-opened index (dir is empty) should be a no-op
        idx.flush().unwrap();
        assert_eq!(idx.search("test").len(), 1);
    }

    #[test]
    fn test_delete_removes_bm25_data() {
        let mut idx = InvertedIndex::new("test");
        idx.index_text("hello world", rid(1));
        assert_eq!(idx.doc_count, 1);
        idx.delete(b"hello").unwrap();
        // delete removes all docs containing "hello" from BM25 tracking
        assert_eq!(idx.doc_count, 0);
    }

    #[test]
    fn test_reindex_updates_bm25() {
        let mut idx = InvertedIndex::new("test");
        idx.index_text("hello", rid(1));
        assert_eq!(idx.doc_count, 1);
        // Re-index the same rid should not inflate doc_count
        idx.index_text("hello world", rid(1));
        assert_eq!(idx.doc_count, 1);
        assert_eq!(*idx.doc_lengths.get(&rid(1)).unwrap(), 2);
    }
}
