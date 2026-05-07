use std::sync::atomic::{AtomicU64, Ordering};
use trident::transactions::{EpochGuard, SnapshotRegistry, VersionStamp, VisibilityWatermark};

#[test]
fn mvcc_version_visibility_respects_create_and_delete_sequences() {
    let version = VersionStamp {
        create_sequence: 10,
        delete_sequence: Some(20),
        commit_timestamp_ms: None,
        expires_at_ms: None,
    };

    assert!(!version.visible_at(VisibilityWatermark { sequence: 9 }));
    assert!(version.visible_at(VisibilityWatermark { sequence: 10 }));
    assert!(!version.visible_at(VisibilityWatermark { sequence: 20 }));
}

#[test]
fn snapshot_registry_pins_gc_horizon_to_oldest_snapshot() {
    let mut registry = SnapshotRegistry::default();
    registry.pin(VisibilityWatermark { sequence: 50 });
    registry.pin(VisibilityWatermark { sequence: 25 });

    assert_eq!(registry.gc_horizon(100).sequence, 25);
    registry.unpin(VisibilityWatermark { sequence: 25 });
    assert_eq!(registry.gc_horizon(100).sequence, 50);
}

#[test]
fn epoch_guard_tracks_active_readers() {
    let active = AtomicU64::new(0);
    {
        let _guard = EpochGuard::enter(&active);
        assert_eq!(active.load(Ordering::SeqCst), 1);
    }
    assert_eq!(active.load(Ordering::SeqCst), 0);
}
