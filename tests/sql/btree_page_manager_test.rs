use trident::index::btree::manager::BTreePageManager;
use trident::store::RecordId;

#[test]
fn btree_page_manager_allocates_inserts_and_splits_leaf_pages() {
    let mut manager = BTreePageManager::new();
    let root = manager.allocate_leaf(1);

    for idx in 0..6 {
        manager
            .insert_leaf(root, format!("k{idx}"), idx, RecordId(idx))
            .unwrap();
    }

    let split = manager.split_leaf(root, 7).unwrap();

    assert_eq!(manager.page_count(), 2);
    assert_eq!(split.left, root);
    assert_eq!(split.fence_key, b"k3");
    assert_eq!(manager.page(root).unwrap().right_sibling, Some(split.right));
    assert_eq!(
        manager.page(split.right).unwrap().find_at(b"k4", u64::MAX),
        Some(RecordId(4))
    );
}
