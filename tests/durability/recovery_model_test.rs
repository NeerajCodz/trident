use praxis::recovery::{CrashFailure, RecoveryAction, RecoveryPlan, RecoveryStage};

#[test]
fn canonical_recovery_plan_matches_kernel_invariant_order() {
    let plan = RecoveryPlan::canonical_startup();

    assert_eq!(
        plan.stages,
        vec![
            RecoveryStage::WalReplay,
            RecoveryStage::ManifestReconcile,
            RecoveryStage::RecordDirectoryRepair,
            RecoveryStage::IndexReplay,
            RecoveryStage::CompactionCleanup,
            RecoveryStage::TierMigrationCleanup,
            RecoveryStage::PublishRecoveredState,
        ]
    );
    plan.validate_deterministic_order().unwrap();
}

#[test]
fn interrupted_compaction_uses_cleanup_action() {
    let plan = RecoveryPlan::deterministic(CrashFailure::CompactionInterrupted);

    assert_eq!(
        plan.actions,
        vec![RecoveryAction::DropUncommittedCompactionOutput]
    );
    plan.validate_deterministic_order().unwrap();
}

#[test]
fn partial_wal_write_uses_torn_suffix_action() {
    let plan = RecoveryPlan::deterministic(CrashFailure::PartialWalWrite);

    assert_eq!(plan.actions, vec![RecoveryAction::IgnoreTornSuffix]);
    plan.validate_deterministic_order().unwrap();
}
