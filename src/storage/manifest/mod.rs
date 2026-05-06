pub mod model;
pub mod store;

pub use model::{
    CheckpointMetadata, ColumnFamilyDescriptor, ColumnFamilyOptions, CompactionJobState,
    CompactionJobStatus, Manifest, SegmentMetadata,
};
pub use store::ManifestStore;

// Aliases for backward compatibility with store module
pub type StorageManifest = Manifest;
pub type StorageManifestStore = ManifestStore;

impl ManifestStore {
    /// Load or create with reasonable defaults (for backward compat)
    pub fn load_or_create_simple(&self) -> crate::errors::Result<Manifest> {
        use crate::config::TridentConfig;
        let default_config = TridentConfig::default();
        self.load_or_create(&default_config)
    }
}
