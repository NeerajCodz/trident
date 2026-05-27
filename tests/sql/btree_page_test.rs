use praxis::index::btree::page::{BTreePage, BTreePageId};
use praxis::store::RecordId;

#[test]
fn btree_page_roundtrips_sorted_leaf_entries() {
    let mut page = BTreePage::leaf(BTreePageId(9), 77);
    page.right_sibling = Some(BTreePageId(10));
    page.insert(b"c".to_vec(), 3, RecordId(3));
    page.insert(b"a".to_vec(), 1, RecordId(1));
    page.insert(b"b".to_vec(), 2, RecordId(2));

    let decoded = BTreePage::from_bytes(&page.to_bytes(), "page").unwrap();

    assert_eq!(decoded.page_id, BTreePageId(9));
    assert_eq!(decoded.page_lsn, 77);
    assert_eq!(decoded.right_sibling, Some(BTreePageId(10)));
    assert_eq!(decoded.entries()[0].key, b"a");
    assert_eq!(decoded.find_at(b"b", 2), Some(RecordId(2)));
}

#[test]
fn btree_page_uses_sequence_visible_lookup() {
    let mut page = BTreePage::leaf(BTreePageId(1), 3);
    page.insert(b"k".to_vec(), 1, RecordId(1));
    page.insert(b"k".to_vec(), 5, RecordId(5));

    let decoded = BTreePage::from_bytes(&page.to_bytes(), "page").unwrap();
    assert_eq!(decoded.find_at(b"k", 1), Some(RecordId(1)));
    assert_eq!(decoded.find_at(b"k", 5), Some(RecordId(5)));
}

#[test]
fn btree_page_rejects_corruption() {
    let mut page = BTreePage::leaf(BTreePageId(1), 1).to_bytes();
    let last = page.len() - 1;
    page[last] ^= 0xff;

    assert!(BTreePage::from_bytes(&page, "page").is_err());
}

#[test]
fn btree_page_tracks_slotted_layout_ghosts_and_overflow_pages() {
    let mut page = BTreePage::leaf(BTreePageId(11), 99);
    page.low_fence = Some(b"a".to_vec());
    page.high_fence = Some(b"z".to_vec());
    page.left_sibling = Some(BTreePageId(10));
    page.right_sibling = Some(BTreePageId(12));
    page.insert(b"k1".to_vec(), 1, RecordId(1));
    page.insert(b"k2".to_vec(), 2, RecordId(2));

    assert!(page.set_overflow_page(b"k2", 2, BTreePageId(30)));
    assert!(page.mark_ghost(b"k1", u64::MAX));
    let layout = page.slotted_layout();

    assert_eq!(layout.slot_directory.len(), 2);
    assert_eq!(layout.overflow_page_count, 1);
    assert!(layout.slot_directory[0].ghost);
    assert_eq!(page.find_at(b"k1", u64::MAX), None);

    let mut decoded = BTreePage::from_bytes(&page.to_bytes(), "page").unwrap();
    assert_eq!(decoded.left_sibling, Some(BTreePageId(10)));
    assert_eq!(decoded.high_fence.as_deref(), Some(b"z".as_slice()));
    assert_eq!(decoded.slotted_layout().overflow_page_count, 1);
    decoded.defragment();
    assert_eq!(decoded.entries().len(), 1);
    assert_eq!(decoded.find_at(b"k2", u64::MAX), Some(RecordId(2)));
}
