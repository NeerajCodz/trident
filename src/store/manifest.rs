use crate::errors::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::fs::File;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StorageManifest {
    pub version: u64,
    pub last_sequence: u64,
    pub primary_segments: Vec<String>,
    pub index_files: HashMap<String, Vec<String>>,
    pub plugin_namespaces: HashMap<String, String>,
    #[serde(default)]
    pub index_compaction_runs: HashMap<String, u64>,
    #[serde(default)]
    pub last_compacted_at_sequence: HashMap<String, u64>,
    #[serde(default)]
    pub maintenance_cycles_run: u64,
    #[serde(default)]
    pub last_maintenance_at_sequence: Option<u64>,
}

impl Default for StorageManifest {
    fn default() -> Self {
        Self {
            version: 1,
            last_sequence: 0,
            primary_segments: Vec::new(),
            index_files: HashMap::new(),
            plugin_namespaces: HashMap::new(),
            index_compaction_runs: HashMap::new(),
            last_compacted_at_sequence: HashMap::new(),
            maintenance_cycles_run: 0,
            last_maintenance_at_sequence: None,
        }
    }
}

pub struct StorageManifestStore {
    path: PathBuf,
}

impl StorageManifestStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn load_or_create(&self) -> Result<StorageManifest> {
        if self.path.exists() {
            let bytes = std::fs::read(&self.path)?;
            Ok(serde_json::from_slice(&bytes)?)
        } else {
            let manifest = StorageManifest::default();
            self.save(&manifest)?;
            Ok(manifest)
        }
    }

    pub fn save(&self, manifest: &StorageManifest) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = self.path.with_extension("tmp");
        let bytes = serde_json::to_vec_pretty(manifest)?;
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(tmp, &self.path)?;
        sync_parent_dir(&self.path)?;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(unix)]
fn sync_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn sync_parent_dir(_path: &Path) -> Result<()> {
    Ok(())
}
