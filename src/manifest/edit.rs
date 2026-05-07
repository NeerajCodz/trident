use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ManifestEditKind {
    AddFile,
    RemoveFile,
    InstallCheckpoint,
    StartCompaction,
    FinishCompaction,
    AbortCompaction,
    AdvanceWal,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ManifestTrackedFile {
    pub path: String,
    pub format: String,
    pub format_version: u16,
    pub checksum: String,
    pub recoverable: bool,
}

impl ManifestTrackedFile {
    pub fn durable(
        path: impl Into<String>,
        format: impl Into<String>,
        format_version: u16,
        checksum: impl Into<String>,
    ) -> Self {
        Self {
            path: path.into(),
            format: format.into(),
            format_version,
            checksum: checksum.into(),
            recoverable: true,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ManifestEdit {
    pub sequence: u64,
    pub kind: ManifestEditKind,
    pub file: Option<ManifestTrackedFile>,
}

impl ManifestEdit {
    pub fn add_file(sequence: u64, file: ManifestTrackedFile) -> Self {
        Self {
            sequence,
            kind: ManifestEditKind::AddFile,
            file: Some(file),
        }
    }
}
