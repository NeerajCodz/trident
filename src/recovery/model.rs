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
    ReadManifest,
    ReplayWal,
    VerifyValueDirectory,
    VerifyIndexes,
    ResolveCompactions,
    InstallCheckpoint,
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
                RecoveryStage::ReadManifest,
                RecoveryStage::ReplayWal,
                RecoveryStage::VerifyValueDirectory,
                RecoveryStage::VerifyIndexes,
                RecoveryStage::ResolveCompactions,
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
}
