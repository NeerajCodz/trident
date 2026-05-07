use trident::config::Compression;
use trident::storage::{ObjectStoreLocator, StorageTier, TieredStoragePolicy};

#[test]
fn tiered_policy_places_data_by_heat_score() {
    let policy = TieredStoragePolicy::default();
    let hot = policy.place(TieredStoragePolicy::heat_score(10_000, 100, 10));
    let cold = policy.place(1);

    assert_eq!(hot.tier, StorageTier::Hot);
    assert_eq!(hot.compression, Compression::None);
    assert_eq!(cold.tier, StorageTier::Cold);
    assert_eq!(cold.compression, Compression::Zstd);
}

#[test]
fn tiered_policy_freezes_to_object_locator() {
    let frozen = TieredStoragePolicy::default().freeze(ObjectStoreLocator {
        bucket: "archive".into(),
        key: "snapshots/1".into(),
    });

    assert_eq!(frozen.tier, StorageTier::Frozen);
    assert_eq!(frozen.object_locator.unwrap().bucket, "archive");
}
