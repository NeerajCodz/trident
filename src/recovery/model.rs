#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CrashFailure {
    ProcessCrash,
    PowerLoss,
    PartialWalWrite,
    TornPage,
    ChecksumMismatch,
    CompactionInterrupted,
    PartialManifestUpdate,
    DiskCorruption,
    ReplicaLag,
    NetworkPartition,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryStage {
    WalReplay,
    ManifestReconcile,
    RecordDirectoryRepair,
    IndexReplay,
    CompactionCleanup,
    TierMigrationCleanup,
    PublishRecoveredState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryAction {
    IgnoreTornSuffix,
    ReplayIdempotently,
    RebuildFromCanonicalStore,
    KeepManifestCommittedFiles,
    DropUncommittedCompactionOutput,
    RequireOperatorRepair,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryGuarantee {
    StructurallyValid,
    SnapshotConsistent,
    ReplaySafe,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryStructureState {
    pub name: String,
    pub versioned: bool,
    pub checksummed: bool,
    pub manifest_tracked: bool,
    pub independently_recoverable: bool,
}

impl RecoveryStructureState {
    pub fn ready(&self) -> bool {
        self.versioned
            && self.checksummed
            && self.manifest_tracked
            && self.independently_recoverable
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryPlan {
    pub failure: CrashFailure,
    pub stages: Vec<RecoveryStage>,
    pub actions: Vec<RecoveryAction>,
    pub guarantees: Vec<RecoveryGuarantee>,
}

impl RecoveryPlan {
    pub fn deterministic(failure: CrashFailure) -> Self {
        let actions = match failure {
            CrashFailure::PartialWalWrite => vec![RecoveryAction::IgnoreTornSuffix],
            CrashFailure::CompactionInterrupted => {
                vec![RecoveryAction::DropUncommittedCompactionOutput]
            }
            CrashFailure::DiskCorruption | CrashFailure::ChecksumMismatch => {
                vec![RecoveryAction::RequireOperatorRepair]
            }
            _ => vec![RecoveryAction::ReplayIdempotently],
        };
        Self {
            failure,
            stages: vec![
                RecoveryStage::WalReplay,
                RecoveryStage::ManifestReconcile,
                RecoveryStage::RecordDirectoryRepair,
                RecoveryStage::IndexReplay,
                RecoveryStage::CompactionCleanup,
                RecoveryStage::TierMigrationCleanup,
                RecoveryStage::PublishRecoveredState,
            ],
            actions,
            guarantees: vec![
                RecoveryGuarantee::StructurallyValid,
                RecoveryGuarantee::SnapshotConsistent,
                RecoveryGuarantee::ReplaySafe,
            ],
        }
    }

    pub fn canonical_startup() -> Self {
        Self::deterministic(CrashFailure::ProcessCrash)
    }

    pub fn invariant_step_names(&self) -> Vec<&'static str> {
        self.stages
            .iter()
            .filter_map(|stage| match stage {
                RecoveryStage::WalReplay => Some("wal_replay"),
                RecoveryStage::ManifestReconcile => Some("manifest_reconcile"),
                RecoveryStage::RecordDirectoryRepair => Some("record_directory_repair"),
                RecoveryStage::IndexReplay => Some("index_replay"),
                RecoveryStage::CompactionCleanup => Some("compaction_cleanup"),
                RecoveryStage::TierMigrationCleanup => Some("tier_migration_cleanup"),
                RecoveryStage::PublishRecoveredState => None,
            })
            .collect()
    }

    pub fn validate_deterministic_order(&self) -> Result<()> {
        KernelInvariantValidator::validate_recovery_plan(&self.invariant_step_names())?;
        Ok(())
    }
}
use crate::errors::Result;
use crate::kernel::KernelInvariantValidator;
