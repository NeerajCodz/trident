use praxis::index::IndexPlugin;
use praxis::{InvertedIndex, RecordId};

#[test]
fn inverted_index_tokenizes_and_finds_terms() {
    let mut index = InvertedIndex::new("text");
    index.index_text("Fast storage, fast search", RecordId(1));
    index.index_text("Graph and vector search", RecordId(2));

    assert_eq!(index.search("FAST"), vec![RecordId(1)]);
    assert_eq!(index.search("search"), vec![RecordId(1), RecordId(2)]);
    assert!(index.term_dictionary().contains(&b"search".to_vec()));

    index.put(b"Manual", RecordId(3)).unwrap();
    assert_eq!(index.get(b"manual"), Some(RecordId(3)));
}

#[test]
fn inverted_index_supports_field_aware_postings() {
    let mut index = InvertedIndex::new("search");
    index.index_field_text("title", "Fast graph storage", RecordId(1));
    index.index_field_text("body", "Fast vector storage", RecordId(2));

    assert_eq!(index.postings("fast"), vec![RecordId(1), RecordId(2)]);
    assert_eq!(index.search_field("title", "fast"), vec![RecordId(1)]);
    assert_eq!(index.field_postings("body", "vector"), vec![RecordId(2)]);
}
