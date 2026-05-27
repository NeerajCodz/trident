use praxis::memory::{AdaptiveMemoryQuota, MemoryManager, MemoryPoolKind, SpillPolicy};

#[test]
fn memory_manager_enforces_hard_quotas_and_spill_policy() {
    let mut manager = MemoryManager::default();
    manager.set_quota(
        MemoryPoolKind::Arena,
        AdaptiveMemoryQuota {
            soft_limit_bytes: 64,
            hard_limit_bytes: 100,
        },
    );

    assert!(manager.reserve(MemoryPoolKind::Arena, 80));
    assert!(!manager.reserve(MemoryPoolKind::Arena, 30));
    assert!(manager.should_spill(
        MemoryPoolKind::Arena,
        SpillPolicy {
            threshold_bytes: 64,
            spill_to_disk: true,
        }
    ));
    manager.release(MemoryPoolKind::Arena, 40);
    assert_eq!(manager.used_bytes(MemoryPoolKind::Arena), 40);
}
