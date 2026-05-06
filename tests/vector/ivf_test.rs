use trident::{IvfFlatIndex, RecordId};

#[test]
fn ivf_exact_fallback_returns_nearest_vectors() {
    let mut index = IvfFlatIndex::new(2, 4);
    assert!(index.insert(RecordId(1), vec![0.0, 0.0]));
    assert!(index.insert(RecordId(2), vec![10.0, 10.0]));
    assert!(!index.insert(RecordId(3), vec![1.0]));

    let hits = index.search_exact(&[1.0, 1.0], 1);
    assert_eq!(hits[0].0, RecordId(1));
    assert_eq!(index.len(), 2);
}
