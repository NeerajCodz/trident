use trident::index::IndexPlugin;
use trident::{InvertedIndex, RecordId};

#[test]
fn inverted_index_tokenizes_and_finds_terms() {
    let mut index = InvertedIndex::new("text");
    index.index_text("Fast storage, fast search", RecordId(1));
    index.index_text("Graph and vector search", RecordId(2));

    assert_eq!(index.search("FAST"), vec![RecordId(1)]);
    assert_eq!(index.search("search"), vec![RecordId(1), RecordId(2)]);

    index.put(b"Manual", RecordId(3)).unwrap();
    assert_eq!(index.get(b"manual"), Some(RecordId(3)));
}
