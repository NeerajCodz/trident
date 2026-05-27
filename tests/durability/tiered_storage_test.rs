use praxis::config::Compression;
use praxis::storage::{
    ObjectStoreLocator, StorageTier, TierHeatSample, TierMigrationManifest, TierMigrationRequest,
    TierMigrationStatus, TieredStoragePolicy,
};
use praxis::store::RecordId;

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

#[test]
fn tiered_migration_manifest_tracks_start_install_and_recovery() {
    let policy = TieredStoragePolicy::default();
    let mut manifest = TierMigrationManifest::default();
    let planned = manifest
        .plan(
            TierMigrationRequest {
                record_id: RecordId(44),
                current_tier: StorageTier::Hot,
                heat: TierHeatSample {
                    reads: 0,
                    writes: 0,
                    age_seconds: 10_000,
                },
                object_locator: None,
            },
            &policy,
        )
        .unwrap();

    assert_eq!(planned.to.tier, StorageTier::Cold);
    assert_eq!(
        manifest.mark_started(planned.migration_id).unwrap().status,
        TierMigrationStatus::Started
    );
    assert_eq!(
        manifest
            .mark_installed(planned.migration_id)
            .unwrap()
            .status,
        TierMigrationStatus::Installed
    );
    assert!(manifest.records[0].manifest_edit_started);
    assert!(manifest.records[0].manifest_edit_installed);
}

#[test]
fn tiered_migration_recovery_aborts_started_uninstalled_records() {
    let policy = TieredStoragePolicy::default();
    let mut manifest = TierMigrationManifest::default();
    let planned = manifest
        .plan(
            TierMigrationRequest {
                record_id: RecordId(45),
                current_tier: StorageTier::Cold,
                heat: TierHeatSample {
                    reads: 0,
                    writes: 0,
                    age_seconds: 10_000,
                },
                object_locator: Some(ObjectStoreLocator {
                    bucket: "archive".into(),
                    key: "records/45".into(),
                }),
            },
            &policy,
        )
        .unwrap();
    manifest.mark_started(planned.migration_id).unwrap();

    let cleanup = manifest.recover_incomplete();

    assert_eq!(cleanup.len(), 1);
    assert_eq!(cleanup[0].status, TierMigrationStatus::Aborted);
    assert_eq!(manifest.records[0].status, TierMigrationStatus::Aborted);
}
